"""Parity tests for Rust-backed coin-op gate and stop policy."""

from __future__ import annotations

from greenfloor.core.coin_ops import (
    coin_op_should_stop,
    evaluate_coin_split_gate,
    evaluate_denomination_readiness,
    is_spendable_wallet_coin,
)
from greenfloor.core.coin_ops.types import DenominationReadiness
from greenfloor.runtime.coin_ops.coins import is_spendable_coin
from greenfloor.runtime.coin_ops.models import SplitDenominationTarget
from greenfloor.runtime.coin_ops.readiness import evaluate_readiness_for_denomination_target


def test_is_spendable_coin_matches_kernel() -> None:
    coin = {"amount": 100, "state": "CONFIRMED"}
    assert is_spendable_coin(coin) is is_spendable_wallet_coin(coin)
    locked = {"amount": 100, "state": "CONFIRMED", "isLocked": True}
    assert is_spendable_coin(locked) is False


def test_evaluate_coin_split_gate_returns_denomination_readiness() -> None:
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
    assert isinstance(readiness, DenominationReadiness)
    assert readiness.ready is True
    assert readiness.reserve_ready is True
    assert readiness.current_count == 2


def test_evaluate_readiness_for_split_denomination_target() -> None:
    coins = [
        {"amount": 100, "state": "CONFIRMED"},
        {"amount": 200, "state": "CONFIRMED"},
    ]
    target = SplitDenominationTarget(
        size_base_units=100,
        target_count=1,
        split_buffer_count=1,
        required_count=2,
    )
    readiness = evaluate_readiness_for_denomination_target(
        asset_scoped_coins=coins,
        asset_id="cat",
        target=target,
    )
    assert readiness is not None
    assert readiness.ready is False
    assert readiness.reserve_ready is True


def test_evaluate_denomination_readiness_split_path() -> None:
    coins = [
        {"amount": 100, "state": "CONFIRMED"},
        {"amount": 200, "state": "CONFIRMED"},
    ]
    readiness = evaluate_denomination_readiness(
        asset_scoped_coins=coins,
        asset_id="cat",
        size_base_units=100,
        required_min_count=2,
    )
    assert readiness.ready is False
    assert readiness.reserve_ready is True


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
