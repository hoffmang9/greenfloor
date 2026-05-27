"""Coin-operation backends: Cloud Wallet GraphQL vs vault signer (coinset + Rust)."""

from __future__ import annotations

import importlib
import logging
from collections.abc import Callable
from dataclasses import dataclass
from typing import Any, Literal, Protocol

from greenfloor.adapters import rust_signer
from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.config.models import (
    MarketConfig,
    ProgramConfig,
    coin_ops_execution_backend,
    prepare_signer_runtime,
)
from greenfloor.core.coin_ops_policy import coin_op_min_amount_mojos
from greenfloor.hex_utils import canonical_is_xch
from greenfloor.runtime.cloud_wallet import assets as cloud_wallet_assets
from greenfloor.runtime.cloud_wallet.coin_ops_execution import combine_coins_with_retry
from greenfloor.runtime.cloud_wallet.coin_ops_models import (
    CoinOpSelectionMode,
    DenominationTarget,
    filter_spendable_for_coin_ops,
)
from greenfloor.runtime.cloud_wallet.coins import (
    classify_resolved_coin_ids_by_asset,
    is_spendable_coin,
    resolve_coin_global_ids,
)
from greenfloor.runtime.coinset_coins import (
    list_unspent_coins_by_receive_address,
    wait_for_coinset_confirmation,
)

CoinOpExecutionBackend = Literal["signer", "cloud_wallet"]


@dataclass(frozen=True, slots=True)
class CoinOpScope:
    market: MarketConfig
    selected_venue: str | None
    execution_backend: CoinOpExecutionBackend
    vault_id: str = ""

    @property
    def allows_daemon_split_combine_prereq(self) -> bool:
        return self.execution_backend == "cloud_wallet"

    def dry_run_reason(self) -> str:
        if self.execution_backend == "signer":
            return "dry_run:signer"
        return "dry_run:cloud_wallet_kms"

    def coin_op_error_prefix(self) -> str:
        if self.execution_backend == "signer":
            return "signer_coin_op_error"
        return "cloud_wallet_coin_op_error"

    def split_submitted_reason(self) -> str:
        if self.execution_backend == "signer":
            return "signer_split_submitted"
        return "cloud_wallet_kms_split_submitted"

    def combine_submitted_reason(self) -> str:
        if self.execution_backend == "signer":
            return "signer_combine_submitted"
        return "cloud_wallet_kms_combine_submitted"

    def combine_prereq_submitted_reason(self, *, exact_match: bool) -> str:
        prefix = (
            "signer_combine_submitted_for_split_prereq"
            if self.execution_backend == "signer"
            else "cloud_wallet_kms_combine_submitted_for_split_prereq"
        )
        suffix = "exact" if exact_match else "with_change"
        return f"{prefix}_{suffix}"


def scope_payload(scope: CoinOpScope) -> dict[str, object]:
    return {
        "market_id": scope.market.market_id,
        "pair": f"{scope.market.base_symbol}:{scope.market.quote_asset}",
        "venue": scope.selected_venue,
        "execution_backend": scope.execution_backend,
        "vault_id": scope.vault_id,
    }


class CoinOpBackend(Protocol):
    @property
    def scope(self) -> CoinOpScope: ...

    @property
    def resolved_asset_id(self) -> str: ...

    @property
    def receive_address(self) -> str: ...

    def list_wallet_coins(self) -> list[dict[str, Any]]: ...

    def list_asset_scoped_coins(self) -> list[dict[str, Any]]: ...

    def filter_spendable(
        self,
        coins: list[dict[str, Any]],
        *,
        canonical_asset_id: str,
        min_coin_amount_mojos: int,
        mode: CoinOpSelectionMode,
        verify_direct_spendable_lookup: bool = False,
    ) -> list[dict[str, Any]]: ...

    def resolve_coin_ids(
        self, wallet_coins: list[dict[str, Any]], raw_coin_ids: list[str]
    ) -> tuple[list[str], list[str]]: ...

    def split_coins(
        self,
        *,
        coin_ids: list[str],
        amount_per_coin: int,
        number_of_coins: int,
        fee_mojos: int,
        initial_coin_ids: set[str] | None = None,
    ) -> dict[str, Any]: ...

    def combine_coins(
        self,
        *,
        number_of_coins: int,
        fee_mojos: int,
        input_coin_ids: list[str] | None,
        largest_first: bool = True,
        target_amount: int | None = None,
    ) -> dict[str, Any]: ...

    def evaluate_denomination_readiness(
        self,
        *,
        asset_id: str,
        size_base_units: int,
        required_min_count: int | None = None,
        max_allowed_count: int | None = None,
    ) -> dict[str, int | bool | str]: ...

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
    ) -> tuple[dict[str, object], dict[str, int | bool | str] | None]: ...


@dataclass(slots=True)
class CloudWalletCoinOpBackend:
    program: ProgramConfig
    market: MarketConfig
    wallet: CloudWalletAdapter
    selected_venue: str | None
    resolved_asset_id: str
    deps: Any

    @property
    def scope(self) -> CoinOpScope:
        return CoinOpScope(
            market=self.market,
            selected_venue=self.selected_venue,
            execution_backend="cloud_wallet",
            vault_id=str(self.wallet.vault_id),
        )

    @property
    def receive_address(self) -> str:
        return str(self.market.receive_address).strip()

    def list_wallet_coins(self) -> list[dict[str, Any]]:
        return self.wallet.list_coins(include_pending=True)

    def list_asset_scoped_coins(self) -> list[dict[str, Any]]:
        return self.wallet.list_coins(asset_id=self.resolved_asset_id, include_pending=True)

    def filter_spendable(
        self,
        coins: list[dict[str, Any]],
        *,
        canonical_asset_id: str,
        min_coin_amount_mojos: int,
        mode: CoinOpSelectionMode,
        verify_direct_spendable_lookup: bool = False,
    ) -> list[dict[str, Any]]:
        return filter_spendable_for_coin_ops(
            coins=coins,
            wallet=self.wallet,
            resolved_asset_id=self.resolved_asset_id,
            canonical_asset_id=canonical_asset_id,
            mode=mode,
            verify_direct_spendable_lookup=verify_direct_spendable_lookup,
        )

    def resolve_coin_ids(
        self, wallet_coins: list[dict[str, Any]], raw_coin_ids: list[str]
    ) -> tuple[list[str], list[str]]:
        return resolve_coin_global_ids(wallet_coins, raw_coin_ids)

    def split_coins(
        self,
        *,
        coin_ids: list[str],
        amount_per_coin: int,
        number_of_coins: int,
        fee_mojos: int,
        initial_coin_ids: set[str] | None = None,
    ) -> dict[str, Any]:
        _ = initial_coin_ids
        return self.wallet.split_coins(
            coin_ids=coin_ids,
            amount_per_coin=amount_per_coin,
            number_of_coins=number_of_coins,
            fee=fee_mojos,
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
        kwargs: dict[str, Any] = {
            "number_of_coins": number_of_coins,
            "fee": fee_mojos,
            "asset_id": self.resolved_asset_id,
            "largest_first": largest_first,
            "input_coin_ids": input_coin_ids,
        }
        if target_amount is not None:
            kwargs["target_amount"] = target_amount
        return combine_coins_with_retry(cloud_wallet=self.wallet, combine_kwargs=kwargs)

    def evaluate_denomination_readiness(
        self,
        *,
        asset_id: str,
        size_base_units: int,
        required_min_count: int | None = None,
        max_allowed_count: int | None = None,
    ) -> dict[str, int | bool | str]:
        from greenfloor.runtime.cloud_wallet.coins import coin_asset_id

        coins = self.wallet.list_coins(include_pending=True)
        spendable = [
            c
            for c in coins
            if is_spendable_coin(c)
            and coin_asset_id(c).lower() == asset_id.strip().lower()
            and int(c.get("amount", 0)) == int(size_base_units)
        ]
        current_count = len(spendable)
        ready = True
        if required_min_count is not None:
            ready = current_count >= int(required_min_count)
        if max_allowed_count is not None:
            ready = ready and current_count <= int(max_allowed_count)
        return {
            "asset_id": asset_id,
            "size_base_units": int(size_base_units),
            "current_count": current_count,
            "required_min_count": int(required_min_count) if required_min_count is not None else -1,
            "max_allowed_count": int(max_allowed_count) if max_allowed_count is not None else -1,
            "ready": ready,
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
        from greenfloor.runtime.cloud_wallet.coin_ops_runtime import coin_op_build_iteration_payload

        return coin_op_build_iteration_payload(
            wallet=self.wallet,
            signature_request_id=operation_id,
            initial_signature_state=operation_state,
            no_wait=no_wait,
            network=network,
            existing_coin_ids=existing_coin_ids,
            iteration=iteration,
            denomination_target=denomination_target,
            readiness_asset_id=readiness_asset_id,
            readiness_kwargs=readiness_kwargs,
            deps=self.deps,
        )


@dataclass(slots=True)
class SignerCoinOpBackend:
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
        initial_coin_ids: set[str] | None,
    ) -> dict[str, Any]:
        config_path = prepare_signer_runtime(self.program)
        request: dict[str, Any] = {
            "receive_address": self.receive_address,
            "asset_id": self.resolved_asset_id.removeprefix("0x"),
            "output_amounts": output_amounts,
            "coin_ids": [value.removeprefix("0x") for value in coin_ids],
            "allow_sub_cat_output": allow_sub_cat_output,
            "fee_mojos": 0,
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
        _ = fee_mojos
        normalized = [value.removeprefix("0x") for value in coin_ids]
        return self._execute_mixed_split(
            output_amounts=[int(amount_per_coin)] * int(number_of_coins),
            coin_ids=normalized,
            allow_sub_cat_output=False,
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
        _ = fee_mojos, largest_first, target_amount
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
            str(c.get("id", c.get("name", ""))).strip() for c in coins if c.get("id") or c.get("name")
        }
        return self._execute_mixed_split(
            output_amounts=output_amounts,
            coin_ids=normalized,
            allow_sub_cat_output=False,
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
        spendable = [
            c
            for c in coins
            if is_spendable_coin(c) and int(c.get("amount", 0)) == int(size_base_units)
        ]
        current_count = len(spendable)
        ready = True
        if required_min_count is not None:
            ready = current_count >= int(required_min_count)
        if max_allowed_count is not None:
            ready = ready and current_count <= int(max_allowed_count)
        return {
            "asset_id": asset_id,
            "size_base_units": int(size_base_units),
            "current_count": current_count,
            "required_min_count": int(required_min_count) if required_min_count is not None else -1,
            "max_allowed_count": int(max_allowed_count) if max_allowed_count is not None else -1,
            "ready": ready,
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


def resolve_signer_asset_id(
    program: ProgramConfig,
    *,
    canonical_asset_id: str,
    symbol_hint: str | None = None,
) -> str:
    _ = symbol_hint
    base = str(canonical_asset_id).strip()
    if canonical_is_xch(base):
        return "xch"
    config_path = prepare_signer_runtime(program)
    resolved = rust_signer.resolve_offer_asset_ids(config_path, base, "xch")
    return str(resolved["base_asset_id"]).strip().lower()


def resolve_coin_op_base_asset_id(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    deps: Any = None,
) -> str:
    """Resolve base asset id for coin-ops (signer coinset or Cloud Wallet catalog)."""
    from greenfloor.runtime.cloud_wallet.coin_ops_runtime import DEFAULT_COIN_OP_DEPS

    if coin_ops_execution_backend(program) == "signer":
        return resolve_signer_asset_id(
            program,
            canonical_asset_id=str(market.base_asset).strip(),
            symbol_hint=str(market.base_symbol).strip() or None,
        )
    coin_deps = deps if deps is not None else DEFAULT_COIN_OP_DEPS
    wallet = coin_deps.new_cloud_wallet_adapter(program)
    return coin_deps.resolve_cloud_wallet_asset_id(
        wallet=wallet,
        canonical_asset_id=str(market.base_asset).strip(),
        symbol_hint=str(market.base_symbol).strip() or None,
        program_home_dir=str(program.home_dir),
    )


def build_coin_op_backend(
    *,
    program: ProgramConfig,
    market: MarketConfig,
    selected_venue: str | None,
    resolved_asset_id: str,
    vault_id: str | None = None,
    deps: Any = None,
) -> CoinOpBackend:
    backend_name = coin_ops_execution_backend(program)
    receive_address = str(market.receive_address).strip()
    if backend_name == "signer":
        if not receive_address:
            raise ValueError("signer_coin_ops_missing_receive_address")
        return SignerCoinOpBackend(
            program=program,
            market=market,
            selected_venue=selected_venue,
            resolved_asset_id=resolved_asset_id,
            receive_address=receive_address,
        )
    from greenfloor.runtime.cloud_wallet.coin_ops_runtime import DEFAULT_COIN_OP_DEPS, CoinOpDeps

    coin_deps = deps if deps is not None else DEFAULT_COIN_OP_DEPS
    wallet = coin_deps.new_cloud_wallet_adapter(program)
    if vault_id and vault_id.strip() and vault_id.strip() != wallet.vault_id:
        from greenfloor.runtime.cloud_wallet import adapter as cloud_wallet_adapter
        from greenfloor.runtime.cloud_wallet.adapter import (
            _require_cloud_wallet_config as require_cloud_wallet_config,
        )

        override_config = require_cloud_wallet_config(program)
        wallet = cloud_wallet_adapter.CloudWalletAdapter(
            cloud_wallet_adapter.CloudWalletConfig(
                base_url=override_config.base_url,
                user_key_id=override_config.user_key_id,
                private_key_pem_path=override_config.private_key_pem_path,
                vault_id=vault_id.strip(),
                network=override_config.network,
            )
        )
    return CloudWalletCoinOpBackend(
        program=program,
        market=market,
        wallet=wallet,
        selected_venue=selected_venue,
        resolved_asset_id=resolved_asset_id,
        deps=coin_deps,
    )
