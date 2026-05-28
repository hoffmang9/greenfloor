"""Rust-backed coin-operation policy bridge.

Each public function is an explicit FFI wrapper (coercion, validation, docstrings).
Do not collapse these into a generic dispatch helper; the repetition is intentional so
each kernel call stays auditable at the Python boundary (see progress.md step 10 handoff).
"""

from __future__ import annotations

from greenfloor.core.coin_ops.kernel_protocol import CoinOpsKernelProtocol
from greenfloor.core.coin_ops.types import (
    BucketSpec,
    CoinOpPlan,
    SplitAutoSelectPlan,
    SplitCoinPlan,
    SplitCombinePrereqPlan,
    SplitSkipPlan,
)
from greenfloor.core.kernel_bridge import import_kernel


def _coin_ops_kernel() -> CoinOpsKernelProtocol:
    return import_kernel()  # type: ignore[return-value]


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
        _coin_ops_kernel().plan_coin_ops(
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
        _coin_ops_kernel().projected_coin_ops_fee_mojos(
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
        _coin_ops_kernel().fee_budget_allows_execution(
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
    allowed, skipped = _coin_ops_kernel().partition_plans_by_budget(
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
    raw = _coin_ops_kernel().compute_bucket_counts_from_coins(
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
    return int(_coin_ops_kernel().coin_op_min_amount_mojos(str(canonical_asset_id)))


def coin_meets_coin_op_min_amount(coin: dict, *, canonical_asset_id: str) -> bool:
    return bool(_coin_ops_kernel().coin_meets_coin_op_min_amount(coin, str(canonical_asset_id)))


def coin_op_target_amount_allowed(*, amount_mojos: int, canonical_asset_id: str) -> bool:
    return bool(
        _coin_ops_kernel().coin_op_target_amount_allowed(
            int(amount_mojos),
            str(canonical_asset_id),
        )
    )


def _require_split_auto_select_plan(value: object) -> SplitAutoSelectPlan:
    if isinstance(value, (SplitCoinPlan, SplitCombinePrereqPlan, SplitSkipPlan)):
        return value
    raise TypeError("kernel returned invalid split auto-select plan")


def select_spendable_coins_for_target_amount(
    *,
    coins: list[dict],
    target_amount: int,
) -> tuple[list[str], int, bool]:
    coin_ids, total, exact = _coin_ops_kernel().select_spendable_coins_for_target_amount(
        coins,
        int(target_amount),
    )
    return [str(coin_id) for coin_id in coin_ids], int(total), bool(exact)


def split_would_create_sub_cat_change(
    *,
    selected_amount_mojos: int,
    required_amount_mojos: int,
    canonical_asset_id: str,
) -> tuple[bool, int]:
    would_create, remainder = _coin_ops_kernel().split_would_create_sub_cat_change(
        int(selected_amount_mojos),
        int(required_amount_mojos),
        str(canonical_asset_id),
    )
    return bool(would_create), int(remainder)


def plan_auto_split_selection(
    *,
    candidate_spendable: list[dict],
    required_amount_mojos: int,
    canonical_asset_id: str,
    profile: str,
    combine_input_cap: int,
    allow_combine_prereq: bool | None = None,
) -> SplitAutoSelectPlan:
    return _require_split_auto_select_plan(
        _coin_ops_kernel().plan_auto_split_selection(
            candidate_spendable,
            int(required_amount_mojos),
            str(canonical_asset_id),
            str(profile),
            int(combine_input_cap),
            allow_combine_prereq,
        )
    )


def plan_auto_combine_inputs(
    *,
    spendable_coins: list[dict],
    number_of_coins: int,
    selection_mode: str,
    target_amount_mojos: int | None = None,
    exclude_coin_ids: set[str] | None = None,
    max_count: int | None = None,
) -> list[str]:
    return [
        str(coin_id)
        for coin_id in _coin_ops_kernel().plan_auto_combine_inputs(
            spendable_coins,
            int(number_of_coins),
            str(selection_mode),
            int(target_amount_mojos) if target_amount_mojos is not None else None,
            exclude_coin_ids,
            int(max_count) if max_count is not None else None,
        )
    ]
