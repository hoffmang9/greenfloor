from greenfloor.core.coin_ops import CoinOpPlan
from greenfloor.core.fee_budget import (
    fee_budget_allows_execution,
    partition_plans_by_budget,
    projected_coin_ops_fee_mojos,
)


def test_projected_coin_ops_fee() -> None:
    fee = projected_coin_ops_fee_mojos(
        plans=[
            CoinOpPlan(op_type="split", size_base_units=1, op_count=3, reason="x"),
            CoinOpPlan(op_type="combine", size_base_units=10, op_count=2, reason="y"),
        ],
        split_fee_mojos=5,
        combine_fee_mojos=7,
    )
    assert fee == (3 * 5) + (2 * 7)


def test_fee_budget_guard() -> None:
    assert fee_budget_allows_execution(
        max_daily_fee_budget_mojos=100,
        spent_today_mojos=40,
        projected_mojos=50,
    )
    assert not fee_budget_allows_execution(
        max_daily_fee_budget_mojos=100,
        spent_today_mojos=60,
        projected_mojos=50,
    )


def test_partition_plans_by_budget_partial_split() -> None:
    allowed, skipped = partition_plans_by_budget(
        plans=[CoinOpPlan(op_type="split", size_base_units=1, op_count=5, reason="r")],
        split_fee_mojos=10,
        combine_fee_mojos=10,
        spent_today_mojos=25,
        max_daily_fee_budget_mojos=55,
    )
    assert len(allowed) == 1
    assert allowed[0].op_count == 3
    assert len(skipped) == 1
    assert skipped[0].op_count == 2
