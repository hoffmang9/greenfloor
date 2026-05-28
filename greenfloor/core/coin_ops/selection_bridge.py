"""FFI bridge for coin-op selection and split/combine input planning (step 11)."""

from __future__ import annotations

from greenfloor.core.coin_ops.kernel_protocol import CoinOpsKernelProtocol
from greenfloor.core.coin_ops.types import (
    CombineInputSelectionMode,
    SplitAutoSelectPlan,
    SplitCoinPlan,
    SplitCombinePrereqPlan,
    SplitPlanningProfile,
    SplitSkipPlan,
)


def _require_split_auto_select_plan(value: object) -> SplitAutoSelectPlan:
    if isinstance(value, SplitCoinPlan | SplitCombinePrereqPlan | SplitSkipPlan):
        return value
    raise TypeError("kernel returned invalid split auto-select plan")


def select_spendable_coins_for_target_amount(
    kernel: CoinOpsKernelProtocol,
    *,
    coins: list[dict],
    target_amount: int,
) -> tuple[list[str], int, bool]:
    coin_ids, total, exact = kernel.select_spendable_coins_for_target_amount(
        coins,
        int(target_amount),
    )
    return [str(coin_id) for coin_id in coin_ids], int(total), bool(exact)


def split_would_create_sub_cat_change(
    kernel: CoinOpsKernelProtocol,
    *,
    selected_amount_mojos: int,
    required_amount_mojos: int,
    canonical_asset_id: str,
) -> tuple[bool, int]:
    would_create, remainder = kernel.split_would_create_sub_cat_change(
        int(selected_amount_mojos),
        int(required_amount_mojos),
        str(canonical_asset_id),
    )
    return bool(would_create), int(remainder)


def plan_auto_split_selection(
    kernel: CoinOpsKernelProtocol,
    *,
    candidate_spendable: list[dict],
    required_amount_mojos: int,
    canonical_asset_id: str,
    profile: SplitPlanningProfile,
    combine_input_cap: int,
    allow_combine_prereq: bool | None = None,
) -> SplitAutoSelectPlan:
    return _require_split_auto_select_plan(
        kernel.plan_auto_split_selection(
            candidate_spendable,
            int(required_amount_mojos),
            str(canonical_asset_id),
            profile,
            int(combine_input_cap),
            allow_combine_prereq,
        )
    )


def plan_auto_combine_inputs(
    kernel: CoinOpsKernelProtocol,
    *,
    spendable_coins: list[dict],
    number_of_coins: int,
    selection_mode: CombineInputSelectionMode,
    target_amount_mojos: int | None = None,
    exclude_coin_ids: set[str] | None = None,
    max_count: int | None = None,
) -> list[str]:
    return [
        str(coin_id)
        for coin_id in kernel.plan_auto_combine_inputs(
            spendable_coins,
            int(number_of_coins),
            selection_mode,
            int(target_amount_mojos) if target_amount_mojos is not None else None,
            exclude_coin_ids,
            int(max_count) if max_count is not None else None,
        )
    ]
