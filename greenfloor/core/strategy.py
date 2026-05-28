from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from typing import Any

from greenfloor.core.cycle import _signer_evaluate_market, _size_counts_to_signer


@dataclass(frozen=True, slots=True)
class MarketState:
    ones: int
    tens: int
    hundreds: int
    xch_price_usd: float | None = None
    bucket_counts_by_size: dict[int, int] | None = None


@dataclass(frozen=True, slots=True)
class StrategyConfig:
    pair: str
    ones_target: int = 5
    tens_target: int = 2
    hundreds_target: int = 1
    target_spread_bps: int | None = None
    min_xch_price_usd: float | None = None
    max_xch_price_usd: float | None = None
    offer_expiry_minutes: int | None = None
    target_counts_by_size: dict[int, int] | None = None


@dataclass(frozen=True, slots=True)
class PlannedAction:
    size: int
    repeat: int
    pair: str
    expiry_unit: str
    expiry_value: int
    cancel_after_create: bool
    reason: str
    target_spread_bps: int | None = None
    side: str = "sell"


def _market_state_payload(state: MarketState) -> dict[str, Any]:
    payload: dict[str, Any] = {
        "ones": int(state.ones),
        "tens": int(state.tens),
        "hundreds": int(state.hundreds),
    }
    if state.xch_price_usd is not None:
        payload["xch_price_usd"] = float(state.xch_price_usd)
    if state.bucket_counts_by_size is not None:
        payload["bucket_counts_by_size"] = _size_counts_to_signer(state.bucket_counts_by_size)
    return payload


def _strategy_config_payload(config: StrategyConfig) -> dict[str, Any]:
    payload: dict[str, Any] = {
        "pair": str(config.pair),
        "ones_target": int(config.ones_target),
        "tens_target": int(config.tens_target),
        "hundreds_target": int(config.hundreds_target),
    }
    if config.target_spread_bps is not None:
        payload["target_spread_bps"] = int(config.target_spread_bps)
    if config.min_xch_price_usd is not None:
        payload["min_xch_price_usd"] = float(config.min_xch_price_usd)
    if config.max_xch_price_usd is not None:
        payload["max_xch_price_usd"] = float(config.max_xch_price_usd)
    if config.offer_expiry_minutes is not None:
        payload["offer_expiry_minutes"] = int(config.offer_expiry_minutes)
    if config.target_counts_by_size is not None:
        payload["target_counts_by_size"] = _size_counts_to_signer(config.target_counts_by_size)
    return payload


def _planned_action_from_payload(payload: dict[str, Any]) -> PlannedAction:
    return PlannedAction(
        size=int(payload["size"]),
        repeat=int(payload["repeat"]),
        pair=str(payload["pair"]),
        expiry_unit=str(payload["expiry_unit"]),
        expiry_value=int(payload["expiry_value"]),
        cancel_after_create=bool(payload["cancel_after_create"]),
        reason=str(payload["reason"]),
        target_spread_bps=(
            int(payload["target_spread_bps"])
            if payload.get("target_spread_bps") is not None
            else None
        ),
        side=str(payload.get("side", "sell")),
    )


def evaluate_market(
    state: MarketState,
    config: StrategyConfig,
    clock: datetime,
) -> list[PlannedAction]:
    _ = clock
    raw_actions = _signer_evaluate_market(
        state=_market_state_payload(state),
        config=_strategy_config_payload(config),
    )
    return [_planned_action_from_payload(item) for item in raw_actions]
