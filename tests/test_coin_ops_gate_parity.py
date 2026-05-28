"""Parity tests for Rust-backed coin-op gate and stop policy."""

from __future__ import annotations

from greenfloor.core.coin_ops import coin_op_should_stop, evaluate_coin_split_gate


def test_evaluate_coin_split_gate_ready() -> None:
    coins = [
        {"amount": 100, "state": "CONFIRMED"},
        {"amount": 100, "state": "CONFIRMED"},
        {"amount": 200, "state": "CONFIRMED"},
    ]
    gate = evaluate_coin_split_gate(
        asset_scoped_coins=coins,
        resolved_asset_id="cat",
        size_base_units=100,
        required_count=2,
    )
    assert gate["ready"] is True
    assert gate["reserve_ready"] is True
    assert gate["current_count"] == 2


def test_coin_op_should_stop_max_iterations() -> None:
    stop, reason = coin_op_should_stop(
        until_ready=True,
        final_readiness={"ready": False},
        coin_ids=[],
        iteration=3,
        max_iterations=3,
    )
    assert stop is True
    assert reason == "max_iterations_reached"
