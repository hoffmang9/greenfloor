"""Manager coin-op policy tests backed by Rust engine gates and ladder helpers."""

from __future__ import annotations

from greenfloor.core.coin_ops import (
    coin_op_should_stop,
    evaluate_coin_combine_gate,
    evaluate_coin_split_gate,
)


def test_split_until_ready_stop_reason_when_gate_satisfied() -> None:
    coins = [
        {"amount": 100, "state": "CONFIRMED"},
        {"amount": 100, "state": "CONFIRMED"},
        {"amount": 200, "state": "CONFIRMED"},
    ]
    readiness = evaluate_coin_split_gate(
        asset_scoped_coins=coins,
        resolved_asset_id="asset",
        size_base_units=100,
        required_count=2,
    )
    stop, reason = coin_op_should_stop(
        until_ready=True,
        readiness_ready=readiness.ready,
        coin_ids=[],
        iteration=1,
        max_iterations=3,
    )
    assert stop is True
    assert reason == "ready"


def test_combine_until_ready_not_ready_when_above_cap() -> None:
    coins = [
        {"amount": 100, "state": "CONFIRMED"},
        {"amount": 100, "state": "CONFIRMED"},
        {"amount": 100, "state": "CONFIRMED"},
    ]
    readiness = evaluate_coin_combine_gate(
        asset_scoped_coins=coins,
        asset_id="asset",
        size_base_units=100,
        max_allowed_count=2,
    )
    assert readiness.ready is False
    stop, reason = coin_op_should_stop(
        until_ready=True,
        readiness_ready=readiness.ready,
        coin_ids=[],
        iteration=3,
        max_iterations=3,
    )
    assert stop is True
    assert reason == "max_iterations_reached"
