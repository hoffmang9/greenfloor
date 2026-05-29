from __future__ import annotations

from dataclasses import replace
from typing import Any, cast

import pytest

from greenfloor.config.models import MarketLadderEntry
from greenfloor.core.offer_bootstrap_policy import (
    BootstrapCoin,
    PlannerLadderRow,
    plan_bootstrap_mixed_outputs,
)
from greenfloor.offer_bootstrap import BootstrapPlanOutcome
from greenfloor.runtime.offer_bootstrap import bootstrap_ladder_entries_for_side
from tests.helpers.config_fixtures import minimal_market_config, minimal_market_with_sell_ladder


def _sample_ladder() -> list[PlannerLadderRow]:
    return [
        PlannerLadderRow(size_base_units=1, target_count=3, split_buffer_count=0),
        PlannerLadderRow(size_base_units=10, target_count=2, split_buffer_count=1),
        PlannerLadderRow(size_base_units=100, target_count=1, split_buffer_count=0),
    ]


def _sample_spendable() -> list[BootstrapCoin]:
    return [
        BootstrapCoin(id="coin-small-1", amount=1),
        BootstrapCoin(id="coin-big", amount=1000),
        BootstrapCoin(id="coin-hundred", amount=100),
    ]


def test_plan_bootstrap_mixed_outputs_builds_deficit_outputs() -> None:
    outcome = plan_bootstrap_mixed_outputs(
        ladder_entries=_sample_ladder(),
        spendable_coins=_sample_spendable(),
    )
    assert outcome.kind == "needs_split"
    assert outcome.plan is not None
    assert outcome.plan.source_coin_id == "coin-big"
    assert sorted(outcome.plan.output_amounts_base_units) == [1, 1, 10, 10, 10]
    assert outcome.plan.total_output_amount == 32


def test_plan_bootstrap_mixed_outputs_returns_ready_when_inventory_satisfied() -> None:
    ladder = [
        PlannerLadderRow(size_base_units=1, target_count=1, split_buffer_count=0),
        PlannerLadderRow(size_base_units=10, target_count=1, split_buffer_count=0),
    ]
    spendable = [
        BootstrapCoin(id="coin-1", amount=1),
        BootstrapCoin(id="coin-10", amount=10),
        BootstrapCoin(id="coin-extra", amount=500),
    ]

    outcome = plan_bootstrap_mixed_outputs(ladder_entries=ladder, spendable_coins=spendable)
    assert outcome.kind == "ready"


def test_plan_bootstrap_mixed_outputs_returns_cannot_fund() -> None:
    ladder = [PlannerLadderRow(size_base_units=10, target_count=2, split_buffer_count=0)]
    spendable = [BootstrapCoin(id="small", amount=5)]

    outcome = plan_bootstrap_mixed_outputs(ladder_entries=ladder, spendable_coins=spendable)
    assert outcome.kind == "cannot_fund"
    assert outcome.total_output_amount == 20


def test_plan_bootstrap_mixed_outputs_single_output_deficit() -> None:
    ladder = [PlannerLadderRow(size_base_units=10, target_count=1, split_buffer_count=0)]
    spendable = [BootstrapCoin(id="coin-big", amount=100)]

    outcome = plan_bootstrap_mixed_outputs(ladder_entries=ladder, spendable_coins=spendable)
    assert outcome.kind == "needs_split"
    assert outcome.plan is not None
    assert outcome.plan.output_amounts_base_units == [10]


def test_plan_bootstrap_mixed_outputs_returns_invalid_ladder_for_negative_fields() -> None:
    ladder = [PlannerLadderRow(size_base_units=-1, target_count=1, split_buffer_count=0)]
    outcome = plan_bootstrap_mixed_outputs(ladder_entries=ladder, spendable_coins=[])
    assert outcome.kind == "invalid_ladder"


def test_plan_bootstrap_mixed_outputs_returns_invalid_coins_for_negative_amount() -> None:
    ladder = [PlannerLadderRow(size_base_units=10, target_count=2, split_buffer_count=0)]
    spendable = [BootstrapCoin(id="coin-a", amount=-1)]
    outcome = plan_bootstrap_mixed_outputs(ladder_entries=ladder, spendable_coins=spendable)
    assert outcome.kind == "invalid_coins"


def test_plan_bootstrap_mixed_outputs_rejects_non_planner_ladder_rows() -> None:
    ladder = cast(Any, [{"size_base_units": 10, "target_count": 1, "split_buffer_count": 0}])
    with pytest.raises(TypeError, match="PlannerLadderRow"):
        plan_bootstrap_mixed_outputs(ladder_entries=ladder, spendable_coins=[])


def test_plan_bootstrap_mixed_outputs_rejects_invalid_coin_amount() -> None:
    ladder = [PlannerLadderRow(size_base_units=10, target_count=2, split_buffer_count=0)]
    spendable = [{"id": "coin-a"}]
    with pytest.raises(ValueError, match="amount"):
        plan_bootstrap_mixed_outputs(ladder_entries=ladder, spendable_coins=spendable)


def test_plan_bootstrap_mixed_outputs_rejects_non_string_coin_id() -> None:
    ladder = [PlannerLadderRow(size_base_units=10, target_count=1, split_buffer_count=0)]
    spendable = [{"id": 42, "amount": 100}]
    with pytest.raises(ValueError, match="id must be a string"):
        plan_bootstrap_mixed_outputs(ladder_entries=ladder, spendable_coins=spendable)


def test_plan_bootstrap_mixed_outputs_requires_kernel_symbol(monkeypatch) -> None:
    import greenfloor.core.kernel_bridge as bridge

    class _Kernel:
        pass

    monkeypatch.setattr(bridge, "bootstrap_kernel", lambda: _Kernel())
    with pytest.raises(RuntimeError, match="plan_bootstrap_mixed_outputs"):
        plan_bootstrap_mixed_outputs(ladder_entries=_sample_ladder(), spendable_coins=[])


def test_phase_result_maps_cannot_fund_to_underfunded_skip() -> None:
    outcome = BootstrapPlanOutcome.cannot_fund(total_output_amount=32)
    result = outcome.to_early_phase_result()
    assert result is not None
    assert result.status == "skipped"
    assert result.reason == "bootstrap_underfunded:total_output_amount=32"


def test_bootstrap_ladder_entries_for_side_normalizes_sell_rows() -> None:
    market = minimal_market_with_sell_ladder(size_base_units=10, target_count=2)
    entries = bootstrap_ladder_entries_for_side(
        side="sell",
        side_ladder=list(market.ladders["sell"]),
        pricing={},
        quote_price=1.0,
        resolved_quote_asset_id="xch",
    )
    assert len(entries) == 1
    assert entries[0].size_base_units == 10
    assert entries[0].target_count == 2


def test_bootstrap_ladder_entries_for_side_normalizes_buy_rows_to_quote_mojos() -> None:
    market = replace(
        minimal_market_config(),
        pricing={
            "quote_unit_mojo_multiplier": 1000,
            "base_unit_mojo_multiplier": 1000,
        },
        ladders={
            "buy": [
                MarketLadderEntry(
                    size_base_units=2,
                    target_count=1,
                    split_buffer_count=0,
                    combine_when_excess_factor=2.0,
                )
            ]
        },
    )
    entries = bootstrap_ladder_entries_for_side(
        side="buy",
        side_ladder=list(market.ladders["buy"]),
        pricing=dict(market.pricing),
        quote_price=1.5,
        resolved_quote_asset_id="quotecat",
    )
    assert len(entries) == 1
    assert entries[0].size_base_units == 3000
    assert entries[0].target_count == 1
