"""Typed PyO3 surface for coin-operation policy bindings."""

from __future__ import annotations

from typing import Protocol

from greenfloor.core.coin_ops.types import (
    BucketSpec,
    CoinOpPlan,
    SplitAutoSelectPlan,
)


class CoinOpsKernelProtocol(Protocol):
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
        profile: str,
        combine_input_cap: int,
        allow_combine_prereq: bool | None,
    ) -> SplitAutoSelectPlan: ...

    def plan_auto_combine_inputs(
        self,
        spendable_coins: list[dict[str, object]],
        number_of_coins: int,
        selection_mode: str,
        target_amount_mojos: int | None,
        exclude_coin_ids: set[str] | None,
        max_count: int | None,
    ) -> list[str]: ...
