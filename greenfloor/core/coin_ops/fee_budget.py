from __future__ import annotations

from greenfloor.core.coin_ops._bridge import _kernel, _require_coin_op_plans
from greenfloor.core.coin_ops.types import CoinOpPlan


def projected_coin_ops_fee_mojos(
    *,
    plans: list[CoinOpPlan],
    split_fee_mojos: int,
    combine_fee_mojos: int,
) -> int:
    return int(
        _kernel().projected_coin_ops_fee_mojos(
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
        _kernel().fee_budget_allows_execution(
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
    allowed, skipped = _kernel().partition_plans_by_budget(
        plans,
        int(split_fee_mojos),
        int(combine_fee_mojos),
        int(spent_today_mojos),
        int(max_daily_fee_budget_mojos),
    )
    return _require_coin_op_plans(allowed), _require_coin_op_plans(skipped)
