from __future__ import annotations

from datetime import UTC, datetime

from greenfloor.core.strategy import (
    MarketState,
    PlannedAction,
    StrategyConfig,
    evaluate_market,
)


def _clock() -> datetime:
    return datetime(2026, 2, 20, 12, 0, 0, tzinfo=UTC)


def test_evaluate_market_returns_no_actions_when_targets_met() -> None:
    state = MarketState(ones=5, tens=2, hundreds=1, xch_price_usd=32.0)
    config = StrategyConfig(pair="xch")

    actions = evaluate_market(state=state, config=config, clock=_clock())

    assert actions == []


def test_evaluate_market_plans_missing_sizes_in_old_script_order() -> None:
    # Old script plans in ones -> tens -> hundreds order.
    state = MarketState(ones=3, tens=1, hundreds=0, xch_price_usd=32.0)
    config = StrategyConfig(
        pair="xch",
        ones_target=5,
        tens_target=2,
        hundreds_target=1,
    )

    actions = evaluate_market(state=state, config=config, clock=_clock())

    assert actions == [
        PlannedAction(
            size=1,
            repeat=2,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=65,
            cancel_after_create=True,
            reason="below_target",
        ),
        PlannedAction(
            size=10,
            repeat=1,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=65,
            cancel_after_create=True,
            reason="below_target",
        ),
        PlannedAction(
            size=100,
            repeat=1,
            pair="xch",
            expiry_unit="minutes",
            expiry_value=65,
            cancel_after_create=True,
            reason="below_target",
        ),
    ]


def test_evaluate_market_uses_usdc_expiry_profile() -> None:
    state = MarketState(ones=4, tens=2, hundreds=1)
    config = StrategyConfig(pair="usdc")

    actions = evaluate_market(state=state, config=config, clock=_clock())

    assert actions == [
        PlannedAction(
            size=1,
            repeat=1,
            pair="usdc",
            expiry_unit="hours",
            expiry_value=24,
            cancel_after_create=True,
            reason="below_target",
        )
    ]


def test_evaluate_market_falls_back_to_xch_expiry_for_unknown_pair() -> None:
    state = MarketState(ones=4, tens=2, hundreds=1)
    config = StrategyConfig(pair="unknown")

    actions = evaluate_market(state=state, config=config, clock=_clock())

    assert actions[0].expiry_unit == "minutes"
    assert actions[0].expiry_value == 65


def test_evaluate_market_xch_requires_price_before_planning() -> None:
    state = MarketState(ones=3, tens=1, hundreds=0, xch_price_usd=None)
    config = StrategyConfig(pair="xch")

    actions = evaluate_market(state=state, config=config, clock=_clock())

    assert actions == []


def test_evaluate_market_xch_plans_when_price_is_available() -> None:
    state = MarketState(ones=3, tens=1, hundreds=0, xch_price_usd=32.25)
    config = StrategyConfig(pair="xch")

    actions = evaluate_market(state=state, config=config, clock=_clock())

    assert [a.size for a in actions] == [1, 10, 100]


def test_evaluate_market_usdc_does_not_require_xch_price() -> None:
    state = MarketState(ones=4, tens=2, hundreds=1, xch_price_usd=None)
    config = StrategyConfig(pair="usdc")

    actions = evaluate_market(state=state, config=config, clock=_clock())

    assert len(actions) == 1
    assert actions[0].pair == "usdc"


def test_evaluate_market_xch_respects_configured_price_band() -> None:
    config = StrategyConfig(
        pair="xch",
        min_xch_price_usd=25.0,
        max_xch_price_usd=35.0,
    )
    out_of_band_low = evaluate_market(
        state=MarketState(ones=0, tens=0, hundreds=0, xch_price_usd=24.9),
        config=config,
        clock=_clock(),
    )
    out_of_band_high = evaluate_market(
        state=MarketState(ones=0, tens=0, hundreds=0, xch_price_usd=35.1),
        config=config,
        clock=_clock(),
    )
    in_band = evaluate_market(
        state=MarketState(ones=0, tens=0, hundreds=0, xch_price_usd=30.0),
        config=config,
        clock=_clock(),
    )
    assert out_of_band_low == []
    assert out_of_band_high == []
    assert len(in_band) == 3


def test_evaluate_market_carries_target_spread_bps_into_actions() -> None:
    actions = evaluate_market(
        state=MarketState(ones=4, tens=2, hundreds=1, xch_price_usd=30.0),
        config=StrategyConfig(pair="xch", target_spread_bps=125),
        clock=_clock(),
    )
    assert len(actions) == 1
    assert actions[0].target_spread_bps == 125
