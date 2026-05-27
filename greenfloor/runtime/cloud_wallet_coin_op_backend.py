"""Cloud Wallet GraphQL coin-operation backend."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from greenfloor.adapters.cloud_wallet import CloudWalletAdapter
from greenfloor.config.models import MarketConfig, ProgramConfig
from greenfloor.runtime.cloud_wallet.coin_ops_execution import combine_coins_with_retry
from greenfloor.runtime.cloud_wallet.coin_ops_models import (
    CoinOpSelectionMode,
    DenominationTarget,
    filter_spendable_for_coin_ops,
)
from greenfloor.runtime.cloud_wallet.coins import is_spendable_coin, resolve_coin_global_ids
from greenfloor.runtime.coin_ops_scope import CoinOpScope


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
