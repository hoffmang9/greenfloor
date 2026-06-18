"""Parity tests for Rust-backed coin-op gate and stop policy."""

from __future__ import annotations

from greenfloor.core.coin_ops import (
    coin_op_should_stop,
    evaluate_coin_combine_gate,
    evaluate_coin_split_gate,
    is_spendable_wallet_coin,
)
from greenfloor.core.coin_ops.types import (
    CombineDenominationReadiness,
    SplitDenominationReadiness,
)


def test_is_spendable_wallet_coin_rejects_locked() -> None:
    coin = {"amount": 100, "state": "CONFIRMED"}
    assert is_spendable_wallet_coin(coin) is True
    locked = {"amount": 100, "state": "CONFIRMED", "isLocked": True}
    assert is_spendable_wallet_coin(locked) is False


def test_evaluate_coin_split_gate_returns_split_readiness() -> None:
    coins = [
        {"amount": 100, "state": "CONFIRMED"},
        {"amount": 100, "state": "CONFIRMED"},
        {"amount": 200, "state": "CONFIRMED"},
    ]
    readiness = evaluate_coin_split_gate(
        asset_scoped_coins=coins,
        resolved_asset_id="cat",
        size_base_units=100,
        required_count=2,
    )
    assert isinstance(readiness, SplitDenominationReadiness)
    assert readiness.ready is True
    assert readiness.reserve_ready is True
    assert readiness.current_count == 2


def test_evaluate_coin_combine_gate_returns_combine_readiness() -> None:
    coins = [
        {"amount": 100, "state": "CONFIRMED"},
        {"amount": 100, "state": "CONFIRMED"},
        {"amount": 100, "state": "CONFIRMED"},
    ]
    readiness = evaluate_coin_combine_gate(
        asset_scoped_coins=coins,
        asset_id="cat",
        size_base_units=100,
        max_allowed_count=2,
    )
    assert isinstance(readiness, CombineDenominationReadiness)
    assert readiness.ready is False
    assert readiness.current_count == 3


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
