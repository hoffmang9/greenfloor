"""Signer (coinset + Rust) coin-operation backend."""

from __future__ import annotations

import importlib
from dataclasses import dataclass
from typing import Any

from greenfloor.adapters import rust_signer
from greenfloor.config.models import MarketConfig, ProgramConfig, prepare_signer_runtime
from greenfloor.core.coin_ops import (
    coin_op_min_amount_mojos,
    combine_denomination_readiness,
    split_denomination_readiness,
)
from greenfloor.runtime.coin_ops.coins import is_spendable_coin
from greenfloor.runtime.coin_ops.models import CoinOpSelectionMode, DenominationTarget
from greenfloor.runtime.coin_ops_scope import CoinOpScope
from greenfloor.runtime.coinset_coins import (
    list_unspent_coins_by_receive_address,
    wait_for_coinset_confirmation,
)


def _operation_id_from_spend_bundle_hex(spend_bundle_hex: str) -> str | None:
    if not spend_bundle_hex:
        return None
    try:
        sdk = importlib.import_module("chia_wallet_sdk")
        raw_hex = (
            spend_bundle_hex[2:] if spend_bundle_hex.lower().startswith("0x") else spend_bundle_hex
        )
        spend_bundle = sdk.SpendBundle.from_bytes(bytes.fromhex(raw_hex))
        return str(sdk.to_hex(spend_bundle.hash()))
    except Exception:
        return None


@dataclass(slots=True)
class SignerCoinOpBackend:
    """BLS/signer coin-ops via Rust mixed-split (coinset listing, no Cloud Wallet fees API)."""

    program: ProgramConfig
    market: MarketConfig
    selected_venue: str | None
    resolved_asset_id: str
    receive_address: str
    no_wait: bool = False

    @property
    def scope(self) -> CoinOpScope:
        return CoinOpScope(
            market=self.market,
            selected_venue=self.selected_venue,
            execution_backend="signer",
            vault_id="signer",
        )

    def list_wallet_coins(self) -> list[dict[str, Any]]:
        return self.list_asset_scoped_coins()

    def list_asset_scoped_coins(self) -> list[dict[str, Any]]:
        return list_unspent_coins_by_receive_address(
            network=str(self.program.app_network),
            receive_address=self.receive_address,
            asset_id=self.resolved_asset_id,
        )

    def filter_spendable(
        self,
        coins: list[dict[str, Any]],
        *,
        canonical_asset_id: str,
        min_coin_amount_mojos: int,
        mode: CoinOpSelectionMode,
        verify_direct_spendable_lookup: bool = False,
    ) -> list[dict[str, Any]]:
        _ = mode, verify_direct_spendable_lookup
        min_amount = max(
            int(min_coin_amount_mojos),
            int(coin_op_min_amount_mojos(canonical_asset_id=canonical_asset_id)),
        )
        return [
            coin
            for coin in coins
            if is_spendable_coin(coin) and int(coin.get("amount", 0)) >= min_amount
        ]

    def resolve_coin_ids(
        self, wallet_coins: list[dict[str, Any]], raw_coin_ids: list[str]
    ) -> tuple[list[str], list[str]]:
        mapping: dict[str, str] = {}
        for coin in wallet_coins:
            coin_id = str(coin.get("id", coin.get("name", ""))).strip().lower()
            if coin_id.startswith("0x"):
                coin_id = coin_id[2:]
            if coin_id:
                mapping[coin_id] = coin_id
            name = str(coin.get("name", "")).strip().lower()
            if name.startswith("0x"):
                name = name[2:]
            if name:
                mapping[name] = coin_id
        resolved: list[str] = []
        unresolved: list[str] = []
        for raw in raw_coin_ids:
            token = str(raw).strip().lower()
            if token.startswith("0x"):
                token = token[2:]
            mapped = mapping.get(token)
            if mapped:
                resolved.append(mapped)
            else:
                unresolved.append(token)
        return resolved, unresolved

    def _execute_mixed_split(
        self,
        *,
        output_amounts: list[int],
        coin_ids: list[str],
        allow_sub_cat_output: bool,
        fee_mojos: int,
        initial_coin_ids: set[str] | None,
    ) -> dict[str, Any]:
        config_path = prepare_signer_runtime(self.program)
        request: dict[str, Any] = {
            "receive_address": self.receive_address,
            "asset_id": self.resolved_asset_id.removeprefix("0x"),
            "output_amounts": output_amounts,
            "coin_ids": [value.removeprefix("0x") for value in coin_ids],
            "allow_sub_cat_output": allow_sub_cat_output,
            "fee_mojos": int(fee_mojos),
            "broadcast": True,
        }
        result = rust_signer.build_mixed_split(config_path, request)
        spend_bundle_hex = str(result.get("spend_bundle_hex", "")).strip()
        operation_id = _operation_id_from_spend_bundle_hex(spend_bundle_hex)
        broadcast_status = str(result.get("broadcast_status", "")).strip() or "submitted"
        payload: dict[str, Any] = {
            "broadcast_status": broadcast_status,
            "operation_id": operation_id,
            "signature_request_id": operation_id or "",
            "status": broadcast_status,
        }
        if self.no_wait or initial_coin_ids is None:
            return payload
        try:
            payload["wait_events"] = wait_for_coinset_confirmation(
                network=str(self.program.app_network),
                receive_address=self.receive_address,
                asset_id=self.resolved_asset_id,
                initial_coin_ids=initial_coin_ids,
                timeout_seconds=15 * 60,
            )
            payload["waited"] = True
        except Exception as exc:
            payload["wait_error"] = str(exc)
        return payload

    def split_coins(
        self,
        *,
        coin_ids: list[str],
        amount_per_coin: int,
        number_of_coins: int,
        fee_mojos: int,
        initial_coin_ids: set[str] | None = None,
    ) -> dict[str, Any]:
        normalized = [value.removeprefix("0x") for value in coin_ids]
        return self._execute_mixed_split(
            output_amounts=[int(amount_per_coin)] * int(number_of_coins),
            coin_ids=normalized,
            allow_sub_cat_output=False,
            fee_mojos=fee_mojos,
            initial_coin_ids=initial_coin_ids,
        )

    def combine_coins(
        self,
        *,
        number_of_coins: int,
        fee_mojos: int,
        input_coin_ids: list[str] | None,
        largest_first: bool = True,
        target_amount: int | None = None,
    ) -> dict[str, Any]:
        """Combine via Rust mixed-split.

        ``largest_first`` and ``target_amount`` are Cloud Wallet-only; the signer path
        always uses explicit ``input_coin_ids`` totals and even output splitting.
        ``fee_mojos`` is forwarded to the Rust mixed-split builder when non-zero.
        """
        _ = largest_first, target_amount
        if not input_coin_ids or len(input_coin_ids) < 2:
            raise ValueError("signer_combine_requires_input_coin_ids")
        normalized = [str(value).strip().lower().removeprefix("0x") for value in input_coin_ids]
        coins = self.list_asset_scoped_coins()
        amount_by_id = {
            str(c.get("id", c.get("name", ""))).strip().lower().removeprefix("0x"): int(
                c.get("amount", 0)
            )
            for c in coins
        }
        total = sum(int(amount_by_id.get(coin_id, 0)) for coin_id in normalized)
        output_count = max(1, int(number_of_coins))
        base = total // output_count
        remainder = total % output_count
        output_amounts = [base] * output_count
        output_amounts[-1] += remainder
        existing_ids = {
            str(c.get("id", c.get("name", ""))).strip()
            for c in coins
            if c.get("id") or c.get("name")
        }
        return self._execute_mixed_split(
            output_amounts=output_amounts,
            coin_ids=normalized,
            allow_sub_cat_output=False,
            fee_mojos=fee_mojos,
            initial_coin_ids=existing_ids,
        )

    def evaluate_denomination_readiness(
        self,
        *,
        asset_id: str,
        size_base_units: int,
        required_min_count: int | None = None,
        max_allowed_count: int | None = None,
    ) -> dict[str, int | bool | str]:
        coins = self.list_asset_scoped_coins()
        if required_min_count is not None and max_allowed_count is None:
            return split_denomination_readiness(
                asset_scoped_coins=coins,
                asset_id=asset_id,
                size_base_units=int(size_base_units),
                required_min_count=int(required_min_count),
            )
        spendable = [
            c
            for c in coins
            if is_spendable_coin(c) and int(c.get("amount", 0)) == int(size_base_units)
        ]
        if max_allowed_count is not None:
            return combine_denomination_readiness(
                asset_id=asset_id,
                size_base_units=int(size_base_units),
                max_allowed_count=int(max_allowed_count),
                matching_count=len(spendable),
            )
        return {
            "asset_id": asset_id,
            "size_base_units": int(size_base_units),
            "current_count": len(spendable),
            "required_min_count": -1,
            "max_allowed_count": -1,
            "ready": True,
        }

    def build_iteration_payload(
        self,
        *,
        operation_id: str,
        operation_state: str,
        no_wait: bool,
        network: str,
        existing_coin_ids: set[str],
        iteration: int,
        readiness_asset_id: str,
        readiness_kwargs: dict[str, int],
        denomination_target: DenominationTarget | None,
    ) -> tuple[dict[str, object], dict[str, int | bool | str] | None]:
        _ = network, existing_coin_ids
        iteration_payload: dict[str, object] = {
            "iteration": iteration,
            "operation_id": operation_id,
            "operation_state": operation_state,
            "signature_request_id": operation_id,
            "signature_state": operation_state,
            "waited": not no_wait,
            "wait_events": [],
        }
        final_readiness = None
        if denomination_target is not None:
            final_readiness = self.evaluate_denomination_readiness(
                asset_id=readiness_asset_id,
                size_base_units=denomination_target.size_base_units,
                **readiness_kwargs,
            )
            iteration_payload["denomination_readiness"] = final_readiness
        return iteration_payload, final_readiness
