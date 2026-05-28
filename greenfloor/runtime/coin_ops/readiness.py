"""Coin-op iteration readiness evaluation and payload shaping."""

from __future__ import annotations

from typing import Any

from greenfloor.core.coin_ops import evaluate_coin_combine_gate, evaluate_coin_split_gate
from greenfloor.core.coin_ops.types import (
    DenominationReadiness,
)
from greenfloor.runtime.coin_ops.models import (
    CombineDenominationTarget,
    DenominationTarget,
    SplitDenominationTarget,
)


def evaluate_readiness_for_denomination_target(
    *,
    asset_scoped_coins: list[dict[str, Any]],
    asset_id: str,
    target: DenominationTarget | None,
) -> DenominationReadiness | None:
    if target is None:
        return None
    if isinstance(target, SplitDenominationTarget):
        return evaluate_coin_split_gate(
            asset_scoped_coins=asset_scoped_coins,
            resolved_asset_id=str(asset_id),
            size_base_units=int(target.size_base_units),
            required_count=int(target.required_count),
        )
    if isinstance(target, CombineDenominationTarget):
        return evaluate_coin_combine_gate(
            asset_scoped_coins=asset_scoped_coins,
            asset_id=str(asset_id),
            size_base_units=int(target.size_base_units),
            max_allowed_count=int(target.combine_threshold_count),
        )
    raise TypeError(f"unsupported denomination target: {type(target)!r}")


def build_coin_op_iteration_payload(
    *,
    operation_id: str,
    operation_state: str,
    no_wait: bool,
    iteration: int,
    readiness_asset_id: str,
    denomination_target: DenominationTarget | None,
    asset_scoped_coins: list[dict[str, Any]],
    readiness: DenominationReadiness | None = None,
    refresh_readiness: bool = False,
) -> tuple[dict[str, object], DenominationReadiness | None]:
    iteration_payload: dict[str, object] = {
        "iteration": iteration,
        "operation_id": operation_id,
        "operation_state": operation_state,
        "signature_request_id": operation_id,
        "signature_state": operation_state,
        "waited": not no_wait,
        "wait_events": [],
    }
    denomination_readiness = readiness
    if refresh_readiness or denomination_readiness is None:
        denomination_readiness = evaluate_readiness_for_denomination_target(
            asset_scoped_coins=asset_scoped_coins,
            asset_id=readiness_asset_id,
            target=denomination_target,
        )
    if denomination_readiness is not None:
        iteration_payload["denomination_readiness"] = denomination_readiness.to_payload()
    return iteration_payload, denomination_readiness
