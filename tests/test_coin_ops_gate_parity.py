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
from greenfloor.runtime.coin_ops.coins import is_spendable_coin
from greenfloor.runtime.coin_ops.models import (
    CombineDenominationTarget,
    SplitDenominationTarget,
)
from greenfloor.runtime.coin_ops.readiness import (
    build_coin_op_iteration_payload,
    evaluate_readiness_for_denomination_target,
)


def test_is_spendable_coin_matches_kernel() -> None:
    coin = {"amount": 100, "state": "CONFIRMED"}
    assert is_spendable_coin(coin) is is_spendable_wallet_coin(coin)
    locked = {"amount": 100, "state": "CONFIRMED", "isLocked": True}
    assert is_spendable_coin(locked) is False


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
    assert isinstance(readiness, SplitDenominationReadiness)
    assert readiness.ready is False
    assert readiness.reserve_ready is True


def test_evaluate_readiness_for_combine_denomination_target() -> None:
    coins = [
        {"amount": 100, "state": "CONFIRMED"},
        {"amount": 100, "state": "CONFIRMED"},
    ]
    target = CombineDenominationTarget(
        size_base_units=100,
        target_count=1,
        combine_when_excess_factor=2.0,
        combine_threshold_count=1,
    )
    readiness = evaluate_readiness_for_denomination_target(
        asset_scoped_coins=coins,
        asset_id="cat",
        target=target,
    )
    assert isinstance(readiness, CombineDenominationReadiness)
    assert readiness.ready is False


def test_build_coin_op_iteration_payload_reuses_readiness_without_refresh() -> None:
    readiness = SplitDenominationReadiness(
        asset_id="cat",
        size_base_units=100,
        required_min_count=2,
        current_count=1,
        larger_reserve_coin_count=1,
        extra_denom_coin_count=0,
        reserve_ready=True,
        ready=False,
    )
    payload, result = build_coin_op_iteration_payload(
        operation_id="op-1",
        operation_state="UNSIGNED",
        no_wait=True,
        iteration=1,
        readiness_asset_id="cat",
        denomination_target=None,
        asset_scoped_coins=[],
        readiness=readiness,
        refresh_readiness=False,
    )
    assert result is readiness
    assert payload["denomination_readiness"] == readiness.to_payload()


def test_build_coin_op_iteration_payload_skips_refresh_when_not_requested() -> None:
    stale = SplitDenominationReadiness(
        asset_id="cat",
        size_base_units=100,
        required_min_count=2,
        current_count=0,
        larger_reserve_coin_count=0,
        extra_denom_coin_count=0,
        reserve_ready=False,
        ready=False,
    )
    payload, result = build_coin_op_iteration_payload(
        operation_id="op-1",
        operation_state="UNSIGNED",
        no_wait=True,
        iteration=1,
        readiness_asset_id="cat",
        denomination_target=None,
        asset_scoped_coins=[],
        readiness=stale,
        refresh_readiness=False,
    )
    assert result is stale
    assert payload["denomination_readiness"] == stale.to_payload()


def test_build_coin_op_iteration_payload_refreshes_when_requested() -> None:
    coins = [
        {"amount": 100, "state": "CONFIRMED"},
        {"amount": 100, "state": "CONFIRMED"},
        {"amount": 200, "state": "CONFIRMED"},
    ]
    stale = SplitDenominationReadiness(
        asset_id="cat",
        size_base_units=100,
        required_min_count=2,
        current_count=0,
        larger_reserve_coin_count=0,
        extra_denom_coin_count=0,
        reserve_ready=False,
        ready=False,
    )
    target = SplitDenominationTarget(
        size_base_units=100,
        target_count=1,
        split_buffer_count=1,
        required_count=2,
    )
    payload, result = build_coin_op_iteration_payload(
        operation_id="op-1",
        operation_state="UNSIGNED",
        no_wait=True,
        iteration=1,
        readiness_asset_id="cat",
        denomination_target=target,
        asset_scoped_coins=coins,
        readiness=stale,
        refresh_readiness=True,
    )
    assert result is not stale
    assert isinstance(result, SplitDenominationReadiness)
    assert result.ready is True
    assert payload["denomination_readiness"] == result.to_payload()


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
