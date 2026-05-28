"""Coin-operation fee budget helpers (Rust-backed)."""

from __future__ import annotations

from greenfloor.core.coin_ops import CoinOpPlan, _import_signer

__all__ = [
    "fee_budget_allows_execution",
    "partition_plans_by_budget",
    "projected_coin_ops_fee_mojos",
]


def projected_coin_ops_fee_mojos(
    *,
    plans: list[CoinOpPlan],
    split_fee_mojos: int,
    combine_fee_mojos: int,
) -> int:
    return int(
        _import_signer().projected_coin_ops_fee_mojos(
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
        _import_signer().fee_budget_allows_execution(
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
    allowed, skipped = _import_signer().partition_plans_by_budget(
        plans,
        int(split_fee_mojos),
        int(combine_fee_mojos),
        int(spent_today_mojos),
        int(max_daily_fee_budget_mojos),
    )
    return _require_coin_op_plans(allowed), _require_coin_op_plans(skipped)


def _require_coin_op_plans(value: object) -> list[CoinOpPlan]:
    if not isinstance(value, list):
        raise TypeError("signer returned non-list result")
    plans: list[CoinOpPlan] = []
    for item in value:
        if not isinstance(item, CoinOpPlan):
            raise TypeError("signer returned non-CoinOpPlan result")
        plans.append(item)
    return plans
