"""Coin-operation policy kernel wiring tests."""

from __future__ import annotations

from greenfloor.core.coin_ops import BucketSpec, CoinOpPlan, plan_coin_ops
from greenfloor.core.coin_ops_policy import (
    coin_meets_coin_op_min_amount,
    coin_op_min_amount_mojos,
    coin_op_target_amount_allowed,
)
from greenfloor.core.fee_budget import (
    fee_budget_allows_execution,
    partition_plans_by_budget,
    projected_coin_ops_fee_mojos,
)
from greenfloor.core.inventory import compute_bucket_counts_from_coins


def test_plan_coin_ops_returns_typed_plans() -> None:
    plans = plan_coin_ops(
        buckets=[
            BucketSpec(
                size_base_units=1,
                target_count=5,
                split_buffer_count=1,
                combine_when_excess_factor=2.0,
                current_count=2,
            )
        ],
        max_operations_per_run=10,
        max_fee_budget_mojos=100,
        split_fee_mojos=1,
        combine_fee_mojos=1,
    )
    assert plans
    assert isinstance(plans[0], CoinOpPlan)
    assert plans[0].op_type == "split"


def test_fee_budget_kernel_wiring() -> None:
    fee = projected_coin_ops_fee_mojos(
        plans=[CoinOpPlan(op_type="split", size_base_units=1, op_count=2, reason="x")],
        split_fee_mojos=5,
        combine_fee_mojos=7,
    )
    assert fee == 10
    assert fee_budget_allows_execution(
        max_daily_fee_budget_mojos=100,
        spent_today_mojos=40,
        projected_mojos=50,
    )
    allowed, skipped = partition_plans_by_budget(
        plans=[CoinOpPlan(op_type="split", size_base_units=1, op_count=5, reason="r")],
        split_fee_mojos=10,
        combine_fee_mojos=10,
        spent_today_mojos=25,
        max_daily_fee_budget_mojos=55,
    )
    assert allowed[0].op_count == 3
    assert skipped[0].op_count == 2


def test_inventory_bucket_kernel_wiring() -> None:
    got = compute_bucket_counts_from_coins(
        coin_amounts_base_units=[1, 1, 2, 10, 100, 99],
        ladder_sizes=[1, 10, 100],
    )
    assert got == {1: 2, 10: 1, 100: 1}


def test_coin_op_min_amount_policy_wiring() -> None:
    cat_id = "0000000000000000000000000000000000000000000000000000000000000001"
    assert coin_op_min_amount_mojos(canonical_asset_id=cat_id) == 1000
    assert not coin_meets_coin_op_min_amount({"amount": 500}, canonical_asset_id=cat_id)
    assert coin_op_target_amount_allowed(amount_mojos=1000, canonical_asset_id=cat_id)
