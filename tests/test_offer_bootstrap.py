from __future__ import annotations

from dataclasses import dataclass

from greenfloor.offer_bootstrap import plan_bootstrap_mixed_outputs


@dataclass
class _Entry:
    size_base_units: int
    target_count: int
    split_buffer_count: int


def test_plan_bootstrap_mixed_outputs_builds_deficit_outputs() -> None:
    ladder = [
        _Entry(size_base_units=1, target_count=3, split_buffer_count=0),
        _Entry(size_base_units=10, target_count=2, split_buffer_count=1),
        _Entry(size_base_units=100, target_count=1, split_buffer_count=0),
    ]
    spendable = [
        {"id": "coin-small-1", "amount": 1},
        {"id": "coin-big", "amount": 1000},
        # This exact 100 coin satisfies the 100-size ladder bucket and avoids
        # creating an unnecessary 100 output in the bootstrap fanout plan.
        {"id": "coin-hundred", "amount": 100},
    ]

    plan = plan_bootstrap_mixed_outputs(sell_ladder=ladder, spendable_coins=spendable)
    assert plan is not None
    assert plan.source_coin_id == "coin-big"
    # Needs two 1s and three 10s (target+buffer for 10 is 3).
    assert sorted(plan.output_amounts_base_units) == [1, 1, 10, 10, 10]
    assert plan.total_output_amount == 32


def test_plan_bootstrap_mixed_outputs_returns_none_when_ready() -> None:
    ladder = [
        _Entry(size_base_units=1, target_count=1, split_buffer_count=0),
        _Entry(size_base_units=10, target_count=1, split_buffer_count=0),
    ]
    spendable = [
        {"id": "coin-1", "amount": 1},
        {"id": "coin-10", "amount": 10},
        {"id": "coin-extra", "amount": 500},
    ]

    assert plan_bootstrap_mixed_outputs(sell_ladder=ladder, spendable_coins=spendable) is None


def test_plan_bootstrap_mixed_outputs_accepts_object_coin_shape() -> None:
    @dataclass
    class _Coin:
        id: str
        amount: int

    ladder = [_Entry(size_base_units=10, target_count=2, split_buffer_count=0)]
    spendable = [_Coin(id="coin-big-object", amount=100)]

    plan = plan_bootstrap_mixed_outputs(sell_ladder=ladder, spendable_coins=spendable)
    assert plan is not None
    assert plan.source_coin_id == "coin-big-object"
    assert plan.output_amounts_base_units == [10, 10]
