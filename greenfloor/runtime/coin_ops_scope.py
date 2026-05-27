"""Coin-operation scope and backend protocol."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Literal, Protocol

from greenfloor.config.models import MarketConfig
from greenfloor.runtime.coin_ops.models import CoinOpSelectionMode, DenominationTarget

CoinOpExecutionBackend = Literal["signer"]


@dataclass(frozen=True, slots=True)
class CoinOpScope:
    market: MarketConfig
    selected_venue: str | None
    execution_backend: CoinOpExecutionBackend
    vault_id: str = ""

    @property
    def allows_daemon_split_combine_prereq(self) -> bool:
        return True

    def dry_run_reason(self) -> str:
        return "dry_run:signer"

    def coin_op_error_prefix(self) -> str:
        return "signer_coin_op_error"

    def split_submitted_reason(self) -> str:
        return "signer_split_submitted"

    def combine_submitted_reason(self) -> str:
        return "signer_combine_submitted"

    def combine_prereq_submitted_reason(self, *, exact_match: bool) -> str:
        suffix = "exact" if exact_match else "with_change"
        return f"signer_combine_submitted_for_split_prereq_{suffix}"


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
