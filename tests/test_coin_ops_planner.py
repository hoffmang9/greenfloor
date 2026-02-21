from greenfloor.core.coin_ops import BucketSpec, plan_coin_ops


def test_plan_splits_when_deficit_exists() -> None:
    plans = plan_coin_ops(
        buckets=[
            BucketSpec(
                size_base_units=1,
                target_count=5,
                split_buffer_count=1,
                combine_when_excess_factor=2.0,
                current_count=2,
            ),
            BucketSpec(
                size_base_units=10,
                target_count=2,
                split_buffer_count=1,
                combine_when_excess_factor=2.0,
                current_count=3,
            ),
        ],
        max_operations_per_run=10,
        max_fee_budget_mojos=100,
        split_fee_mojos=1,
        combine_fee_mojos=1,
    )
    assert plans
    assert plans[0].op_type == "split"
    assert plans[0].size_base_units == 1


def test_plan_combines_only_when_no_deficits() -> None:
    plans = plan_coin_ops(
        buckets=[
            BucketSpec(
                size_base_units=1,
                target_count=5,
                split_buffer_count=1,
                combine_when_excess_factor=2.0,
                current_count=12,
            ),
        ],
        max_operations_per_run=4,
        max_fee_budget_mojos=10,
        split_fee_mojos=1,
        combine_fee_mojos=1,
    )
    assert plans
    assert plans[0].op_type == "combine"
