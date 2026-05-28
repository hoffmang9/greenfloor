"""Rust-backed coin-operation policy bridge.

Each public function is an explicit FFI wrapper (coercion, validation, docstrings).
Do not collapse these into a generic dispatch helper; the repetition is intentional so
each kernel call stays auditable at the Python boundary (see progress.md step 10 handoff).
"""

from __future__ import annotations

from typing import Any

from greenfloor.core import kernel_bridge
from greenfloor.core.coin_ops.selection_bridge import (
    plan_auto_combine_inputs as _plan_auto_combine_inputs,
)
from greenfloor.core.coin_ops.selection_bridge import (
    plan_auto_split_selection as _plan_auto_split_selection,
)
from greenfloor.core.coin_ops.selection_bridge import (
    select_spendable_coins_for_target_amount as _select_spendable_coins_for_target_amount,
)
from greenfloor.core.coin_ops.selection_bridge import (
    split_would_create_sub_cat_change as _split_would_create_sub_cat_change,
)
from greenfloor.core.coin_ops.types import (
    BucketSpec,
    CoinOpPlan,
    CombineDenominationReadiness,
    CombineInputSelectionMode,
    SplitAutoSelectPlan,
    SplitDenominationReadiness,
    SplitPlanningProfile,
)


def _require_coin_op_plans(value: object) -> list[CoinOpPlan]:
    if not isinstance(value, list):
        raise TypeError("kernel returned non-list result")
    plans: list[CoinOpPlan] = []
    for item in value:
        if not isinstance(item, CoinOpPlan):
            raise TypeError("kernel returned non-CoinOpPlan result")
        plans.append(item)
    return plans


def plan_coin_ops(
    *,
    buckets: list[BucketSpec],
    max_operations_per_run: int,
    max_fee_budget_mojos: int,
    split_fee_mojos: int,
    combine_fee_mojos: int,
) -> list[CoinOpPlan]:
    return _require_coin_op_plans(
        kernel_bridge.coin_ops_kernel().plan_coin_ops(
            buckets,
            int(max_operations_per_run),
            int(max_fee_budget_mojos),
            int(split_fee_mojos),
            int(combine_fee_mojos),
        )
    )


def projected_coin_ops_fee_mojos(
    *,
    plans: list[CoinOpPlan],
    split_fee_mojos: int,
    combine_fee_mojos: int,
) -> int:
    return int(
        kernel_bridge.coin_ops_kernel().projected_coin_ops_fee_mojos(
            plans,
            int(split_fee_mojos),
            int(combine_fee_mojos),
        )
    )


def fee_budget_allows_execution(
    *,
    max_daily_fee_budget_mojos: int,
    spent_today_mojos: int,
    projected_mojos: int,
) -> bool:
    return bool(
        kernel_bridge.coin_ops_kernel().fee_budget_allows_execution(
            int(max_daily_fee_budget_mojos),
            int(spent_today_mojos),
            int(projected_mojos),
        )
    )


def partition_plans_by_budget(
    *,
    plans: list[CoinOpPlan],
    split_fee_mojos: int,
    combine_fee_mojos: int,
    spent_today_mojos: int,
    max_daily_fee_budget_mojos: int,
) -> tuple[list[CoinOpPlan], list[CoinOpPlan]]:
    """Split plans into executable and overflow-by-budget plans.

    Preserves input order. If budget is unlimited (<=0), all plans are executable.
    Can split a plan by op_count if only partial operations fit.
    """
    allowed, skipped = kernel_bridge.coin_ops_kernel().partition_plans_by_budget(
        plans,
        int(split_fee_mojos),
        int(combine_fee_mojos),
        int(spent_today_mojos),
        int(max_daily_fee_budget_mojos),
    )
    return _require_coin_op_plans(allowed), _require_coin_op_plans(skipped)


def compute_bucket_counts_from_coins(
    *,
    coin_amounts_base_units: list[int],
    ladder_sizes: list[int],
) -> dict[int, int]:
    """Compute per-size bucket counts from available coin amounts.

    V1 logic is exact-match by ladder size to keep behavior deterministic and auditable.
    """
    raw = kernel_bridge.coin_ops_kernel().compute_bucket_counts_from_coins(
        [int(amount) for amount in coin_amounts_base_units],
        [int(size) for size in ladder_sizes],
    )
    if not isinstance(raw, dict):
        raise TypeError("kernel returned non-dict result")
    return {int(key): int(value) for key, value in raw.items()}


def coin_op_min_amount_mojos(*, canonical_asset_id: str) -> int:
    # Temporary workaround for the upstream Cloud Wallet / ent-wallet asset-scope
    # bug documented in docs/ent-wallet-upstream-byc-coin-query-issue.md.
    # Ignore sub-1-CAT dust during local split/combine candidate selection so
    # tiny stray rows do not get pulled into operational coin management.
    return int(kernel_bridge.coin_ops_kernel().coin_op_min_amount_mojos(str(canonical_asset_id)))


def coin_meets_coin_op_min_amount(coin: dict, *, canonical_asset_id: str) -> bool:
    return bool(
        kernel_bridge.coin_ops_kernel().coin_meets_coin_op_min_amount(coin, str(canonical_asset_id))
    )


def coin_op_target_amount_allowed(*, amount_mojos: int, canonical_asset_id: str) -> bool:
    return bool(
        kernel_bridge.coin_ops_kernel().coin_op_target_amount_allowed(
            int(amount_mojos),
            str(canonical_asset_id),
        )
    )


def select_spendable_coins_for_target_amount(
    *,
    coins: list[dict],
    target_amount: int,
) -> tuple[list[str], int, bool]:
    return _select_spendable_coins_for_target_amount(
        kernel_bridge.coin_ops_kernel(),
        coins=coins,
        target_amount=target_amount,
    )


def split_would_create_sub_cat_change(
    *,
    selected_amount_mojos: int,
    required_amount_mojos: int,
    canonical_asset_id: str,
) -> tuple[bool, int]:
    return _split_would_create_sub_cat_change(
        kernel_bridge.coin_ops_kernel(),
        selected_amount_mojos=selected_amount_mojos,
        required_amount_mojos=required_amount_mojos,
        canonical_asset_id=canonical_asset_id,
    )


def plan_auto_split_selection(
    *,
    candidate_spendable: list[dict],
    required_amount_mojos: int,
    canonical_asset_id: str,
    profile: SplitPlanningProfile,
    combine_input_cap: int,
    allow_combine_prereq: bool | None = None,
) -> SplitAutoSelectPlan:
    return _plan_auto_split_selection(
        kernel_bridge.coin_ops_kernel(),
        candidate_spendable=candidate_spendable,
        required_amount_mojos=required_amount_mojos,
        canonical_asset_id=canonical_asset_id,
        profile=profile,
        combine_input_cap=combine_input_cap,
        allow_combine_prereq=allow_combine_prereq,
    )


def plan_auto_combine_inputs(
    *,
    spendable_coins: list[dict],
    number_of_coins: int,
    selection_mode: CombineInputSelectionMode,
    target_amount_mojos: int | None = None,
    exclude_coin_ids: set[str] | None = None,
    max_count: int | None = None,
) -> list[str]:
    return _plan_auto_combine_inputs(
        kernel_bridge.coin_ops_kernel(),
        spendable_coins=spendable_coins,
        number_of_coins=number_of_coins,
        selection_mode=selection_mode,
        target_amount_mojos=target_amount_mojos,
        exclude_coin_ids=exclude_coin_ids,
        max_count=max_count,
    )


def is_spendable_wallet_coin(coin: dict[str, Any]) -> bool:
    return bool(kernel_bridge.coin_ops_kernel().is_spendable_wallet_coin(coin))


def evaluate_coin_split_gate(
    *,
    asset_scoped_coins: list[dict[str, Any]],
    resolved_asset_id: str,
    size_base_units: int,
    required_count: int,
) -> SplitDenominationReadiness:
    result = kernel_bridge.coin_ops_kernel().evaluate_coin_split_gate(
        asset_scoped_coins,
        str(resolved_asset_id),
        int(size_base_units),
        int(required_count),
    )
    if not isinstance(result, SplitDenominationReadiness):
        raise TypeError("kernel returned non-SplitDenominationReadiness result")
    return result


def evaluate_coin_combine_gate(
    *,
    asset_scoped_coins: list[dict[str, Any]],
    asset_id: str,
    size_base_units: int,
    max_allowed_count: int,
) -> CombineDenominationReadiness:
    result = kernel_bridge.coin_ops_kernel().evaluate_coin_combine_gate(
        asset_scoped_coins,
        str(asset_id),
        int(size_base_units),
        int(max_allowed_count),
    )
    if not isinstance(result, CombineDenominationReadiness):
        raise TypeError("kernel returned non-CombineDenominationReadiness result")
    return result


def coin_op_should_stop(
    *,
    until_ready: bool,
    readiness_ready: bool | None,
    coin_ids: list[str],
    iteration: int,
    max_iterations: int,
) -> tuple[bool, str]:
    stop, reason = kernel_bridge.coin_ops_kernel().coin_op_should_stop(
        bool(until_ready),
        readiness_ready,
        bool(coin_ids),
        int(iteration),
        int(max_iterations),
    )
    return bool(stop), str(reason)
