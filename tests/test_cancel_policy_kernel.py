"""Cancel-policy kernel parity tests."""

from __future__ import annotations

import pytest

from greenfloor.core.cancel_policy import (
    CancelPolicyDecision,
    abs_move_bps,
    cancel_move_threshold_bps,
    evaluate_cancel_policy_decision,
)


def test_abs_move_bps_positive_move() -> None:
    result = abs_move_bps(110.0, 100.0)
    assert result is not None
    assert abs(result - 1000.0) < 0.01


@pytest.mark.parametrize(
    ("current", "previous"),
    [(None, 100.0), (100.0, None), (0.0, 100.0), (100.0, 0.0)],
)
def test_abs_move_bps_rejects_invalid_inputs(current, previous) -> None:
    assert abs_move_bps(current, previous) is None


def test_cancel_move_threshold_bps_prefers_market_override() -> None:
    assert cancel_move_threshold_bps(market_threshold=100, env_threshold=250) == 100


def test_cancel_move_threshold_bps_uses_env_when_market_missing() -> None:
    assert cancel_move_threshold_bps(env_threshold=250) == 250
    assert cancel_move_threshold_bps() == 500


@pytest.mark.parametrize(
    ("quote_asset_type", "stable_vs_unstable", "current", "previous", "expected"),
    [
        (
            "stable",
            True,
            30.0,
            25.0,
            CancelPolicyDecision(
                eligible=False,
                triggered=False,
                reason="not_unstable_leg_market",
                move_bps=2000.0,
                threshold_bps=500,
            ),
        ),
        (
            "unstable",
            False,
            45.0,
            30.0,
            CancelPolicyDecision(
                eligible=False,
                triggered=False,
                reason="not_stable_vs_unstable_market",
                move_bps=5000.0,
                threshold_bps=500,
            ),
        ),
    ],
)
def test_evaluate_cancel_policy_decision_branches(
    quote_asset_type: str,
    stable_vs_unstable: bool,
    current: float,
    previous: float,
    expected: CancelPolicyDecision,
) -> None:
    decision = evaluate_cancel_policy_decision(
        quote_asset_type=quote_asset_type,
        cancel_policy_stable_vs_unstable=stable_vs_unstable,
        current_xch_price_usd=current,
        previous_xch_price_usd=previous,
    )
    assert decision.eligible == expected.eligible
    assert decision.triggered == expected.triggered
    assert decision.reason == expected.reason
    assert decision.threshold_bps == expected.threshold_bps
    assert decision.move_bps == expected.move_bps


def test_evaluate_cancel_policy_decision_price_move_below_threshold() -> None:
    decision = evaluate_cancel_policy_decision(
        quote_asset_type="unstable",
        cancel_policy_stable_vs_unstable=True,
        current_xch_price_usd=30.2,
        previous_xch_price_usd=30.0,
    )
    assert decision.eligible is True
    assert decision.triggered is False
    assert decision.reason == "price_move_below_threshold"
    assert decision.move_bps == pytest.approx(66.666, rel=1e-3)
    assert decision.threshold_bps == 500


def test_evaluate_cancel_policy_decision_uses_market_threshold() -> None:
    decision = evaluate_cancel_policy_decision(
        quote_asset_type="unstable",
        cancel_policy_stable_vs_unstable=True,
        current_xch_price_usd=30.6,
        previous_xch_price_usd=30.0,
        market_threshold=100,
    )
    assert decision.triggered is True
    assert decision.threshold_bps == 100
