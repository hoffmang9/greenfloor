"""Parity tests for Rust-backed coin-op gate and stop policy."""

from __future__ import annotations

from greenfloor.core.coin_ops import (
    coin_op_should_stop,
    evaluate_coin_split_gate,
    is_spendable_wallet_coin,
    split_denomination_readiness,
)
from greenfloor.runtime.coin_ops.coins import is_spendable_coin


def test_is_spendable_coin_matches_kernel() -> None:
    coin = {"amount": 100, "state": "CONFIRMED"}
    assert is_spendable_coin(coin) is is_spendable_wallet_coin(coin)
    locked = {"amount": 100, "state": "CONFIRMED", "isLocked": True}
    assert is_spendable_coin(locked) is False


def test_split_denomination_readiness_matches_gate() -> None:
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
    payload = split_denomination_readiness(
        asset_scoped_coins=coins,
        asset_id="cat",
        size_base_units=100,
        required_min_count=2,
    )
    assert payload == gate.to_readiness_payload()
    assert payload["ready"] is True


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
    assert gate.ready is True
    assert gate.reserve_ready is True
    assert gate.current_count == 2


def test_coin_op_should_stop_max_iterations() -> None:
    stop, reason = coin_op_should_stop(
        until_ready=True,
        readiness_ready=False,
        coin_ids=[],
        iteration=3,
        max_iterations=3,
    )
    assert stop is True
    assert reason == "max_iterations_reached"
