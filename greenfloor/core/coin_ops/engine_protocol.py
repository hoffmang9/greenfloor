"""Typed PyO3 surface for coin-operation policy bindings."""

from __future__ import annotations

from typing import Any, Protocol

from greenfloor.core.coin_ops.types import (
    BucketSpec,
    CoinOpPlan,
    CombineDenominationReadiness,
    CombineInputSelectionMode,
    SplitAutoSelectPlan,
    SplitDenominationReadiness,
    SplitPlanningProfile,
)


class CoinOpsEngineProtocol(Protocol):
    def plan_coin_ops(
        self,
        buckets: list[BucketSpec],
        max_operations_per_run: int,
        max_fee_budget_mojos: int,
        split_fee_mojos: int,
        combine_fee_mojos: int,
    ) -> list[CoinOpPlan]: ...

    def projected_coin_ops_fee_mojos(
        self,
        plans: list[CoinOpPlan],
        split_fee_mojos: int,
        combine_fee_mojos: int,
    ) -> int: ...

    def fee_budget_allows_execution(
        self,
        max_daily_fee_budget_mojos: int,
        spent_today_mojos: int,
        projected_mojos: int,
    ) -> bool: ...

    def partition_plans_by_budget(
        self,
        plans: list[CoinOpPlan],
        split_fee_mojos: int,
        combine_fee_mojos: int,
        spent_today_mojos: int,
        max_daily_fee_budget_mojos: int,
    ) -> tuple[list[CoinOpPlan], list[CoinOpPlan]]: ...

    def compute_bucket_counts_from_coins(
        self,
        coin_amounts_base_units: list[int],
        ladder_sizes: list[int],
    ) -> dict[int, int]: ...

    def coin_op_min_amount_mojos(self, canonical_asset_id: str) -> int: ...

    def coin_meets_coin_op_min_amount(
        self, coin: dict[str, object], canonical_asset_id: str
    ) -> bool: ...

    def coin_op_target_amount_allowed(self, amount_mojos: int, canonical_asset_id: str) -> bool: ...

    def select_spendable_coins_for_target_amount(
        self,
        coins: list[dict[str, object]],
        target_amount: int,
    ) -> tuple[list[str], int, bool]: ...

    def split_would_create_sub_cat_change(
        self,
        selected_amount_mojos: int,
        required_amount_mojos: int,
        canonical_asset_id: str,
    ) -> tuple[bool, int]: ...

    def plan_auto_split_selection(
        self,
        candidate_spendable: list[dict[str, object]],
        required_amount_mojos: int,
        canonical_asset_id: str,
        profile: SplitPlanningProfile,
        combine_input_cap: int,
        allow_combine_prereq: bool | None,
    ) -> SplitAutoSelectPlan: ...

    def plan_auto_combine_inputs(
        self,
        spendable_coins: list[dict[str, object]],
        number_of_coins: int,
        selection_mode: CombineInputSelectionMode,
        target_amount_mojos: int | None,
        exclude_coin_ids: set[str] | None,
        max_count: int | None,
    ) -> list[str]: ...

    def is_spendable_wallet_coin(self, coin: dict[str, Any]) -> bool: ...

    def evaluate_coin_split_gate(
        self,
        asset_scoped_coins: list[dict[str, Any]],
        resolved_asset_id: str,
        size_base_units: int,
        required_count: int,
    ) -> SplitDenominationReadiness: ...

    def evaluate_coin_combine_gate(
        self,
        asset_scoped_coins: list[dict[str, Any]],
        asset_id: str,
        size_base_units: int,
        max_allowed_count: int,
    ) -> CombineDenominationReadiness: ...

    def coin_op_should_stop(
        self,
        until_ready: bool,
        final_readiness_ready: bool | None,
        has_explicit_coin_ids: bool,
        iteration: int,
        max_iterations: int,
    ) -> tuple[bool, str]: ...

    def effective_sell_bucket_counts_for_coin_ops(
        self,
        sell_ladder: list[Any],
        wallet_bucket_counts: dict[int, int],
        active_sell_offer_counts_by_size: dict[int, int] | None,
        newly_executed_sell_offer_counts_by_size: dict[int, int] | None,
    ) -> dict[int, int]: ...
