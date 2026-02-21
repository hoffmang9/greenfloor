from __future__ import annotations

from greenfloor.core.coin_ops import CoinOpPlan


def projected_coin_ops_fee_mojos(
    *,
    plans: list[CoinOpPlan],
    split_fee_mojos: int,
    combine_fee_mojos: int,
) -> int:
    total = 0
    for plan in plans:
        per_op_fee = split_fee_mojos if plan.op_type == "split" else combine_fee_mojos
        total += max(0, plan.op_count) * max(0, per_op_fee)
    return total


def fee_budget_allows_execution(
    *,
    max_daily_fee_budget_mojos: int,
    spent_today_mojos: int,
    projected_mojos: int,
) -> bool:
    if max_daily_fee_budget_mojos <= 0:
        return True
    return spent_today_mojos + projected_mojos <= max_daily_fee_budget_mojos


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
    if max_daily_fee_budget_mojos <= 0:
        return plans[:], []

    remaining = max(0, max_daily_fee_budget_mojos - max(0, spent_today_mojos))
    allowed: list[CoinOpPlan] = []
    skipped: list[CoinOpPlan] = []

    for plan in plans:
        per_op = split_fee_mojos if plan.op_type == "split" else combine_fee_mojos
        per_op = max(0, per_op)
        if plan.op_count <= 0:
            continue
        if per_op == 0:
            allowed.append(plan)
            continue
        affordable_ops = remaining // per_op
        if affordable_ops <= 0:
            skipped.append(plan)
            continue
        if affordable_ops >= plan.op_count:
            allowed.append(plan)
            remaining -= plan.op_count * per_op
            continue
        # Partial fit.
        allowed.append(
            CoinOpPlan(
                op_type=plan.op_type,
                size_base_units=plan.size_base_units,
                op_count=int(affordable_ops),
                reason=plan.reason,
            )
        )
        skipped.append(
            CoinOpPlan(
                op_type=plan.op_type,
                size_base_units=plan.size_base_units,
                op_count=plan.op_count - int(affordable_ops),
                reason="fee_budget_partial_overflow",
            )
        )
        remaining = 0

    return allowed, skipped
