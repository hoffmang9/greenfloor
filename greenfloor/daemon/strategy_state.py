"""Daemon strategy evaluation helpers."""

from __future__ import annotations

from datetime import datetime
from typing import Any

from greenfloor.core.strategy import MarketState, PlannedAction, StrategyConfig, evaluate_market
from greenfloor.daemon.market_helpers import _market_pricing, _normalize_strategy_pair


def _normalize_target_counts(
    raw: dict,
    *,
    defaults: dict[int, int] | None = None,
) -> dict[int, int]:
    """Normalize a {size: target_count} mapping from config or ladder data.

    Drops non-positive sizes, clamps negative targets to zero, and falls back
    to *defaults* when the result would otherwise be empty.
    """
    out = {int(k): max(0, int(v)) for k, v in raw.items() if int(k) > 0}
    if not out and defaults:
        return dict(defaults)
    return out


def _strategy_config_from_market(market) -> StrategyConfig:
    sell_ladder = market.ladders.get("sell", [])
    targets_by_size = {int(e.size_base_units): int(e.target_count) for e in sell_ladder}
    pricing = _market_pricing(market)

    def _to_int(value: Any) -> int | None:
        if value is None:
            return None
        try:
            parsed = int(value)
        except (TypeError, ValueError):
            return None
        return parsed

    def _to_float(value: Any) -> float | None:
        if value is None:
            return None
        try:
            parsed = float(value)
        except (TypeError, ValueError):
            return None
        return parsed

    normalized_targets = _normalize_target_counts(targets_by_size, defaults={1: 5, 10: 2, 100: 1})

    return StrategyConfig(
        pair=_normalize_strategy_pair(market.quote_asset),
        ones_target=int(normalized_targets.get(1, 0)),
        tens_target=int(normalized_targets.get(10, 0)),
        hundreds_target=int(normalized_targets.get(100, 0)),
        target_spread_bps=_to_int(pricing.get("strategy_target_spread_bps")),
        min_xch_price_usd=_to_float(pricing.get("strategy_min_xch_price_usd")),
        max_xch_price_usd=_to_float(pricing.get("strategy_max_xch_price_usd")),
        offer_expiry_minutes=_to_int(pricing.get("strategy_offer_expiry_minutes")),
        target_counts_by_size=normalized_targets,
    )


def _strategy_config_for_side(*, market: Any, side: str) -> StrategyConfig:
    ladders = getattr(market, "ladders", {}) or {}
    side_ladder = list(ladders.get(side, []) or []) if isinstance(ladders, dict) else []
    targets_by_size = {int(e.size_base_units): int(e.target_count) for e in side_ladder}
    pricing = _market_pricing(market)

    expiry_minutes_raw = pricing.get("strategy_offer_expiry_minutes")
    expiry_minutes: int | None = None
    if expiry_minutes_raw is not None:
        try:
            expiry_minutes = int(expiry_minutes_raw)
        except (TypeError, ValueError):
            expiry_minutes = None

    normalized_targets = _normalize_target_counts(targets_by_size)

    return StrategyConfig(
        pair=_normalize_strategy_pair(market.quote_asset),
        ones_target=int(normalized_targets.get(1, 0)),
        tens_target=int(normalized_targets.get(10, 0)),
        hundreds_target=int(normalized_targets.get(100, 0)),
        offer_expiry_minutes=expiry_minutes,
        target_counts_by_size=normalized_targets,
    )


def _strategy_state_from_bucket_counts(
    bucket_counts: dict[int, int],
    *,
    xch_price_usd: float | None,
) -> MarketState:
    normalized_bucket_counts = {int(size): int(count) for size, count in bucket_counts.items()}
    return MarketState(
        ones=int(normalized_bucket_counts.get(1, 0)),
        tens=int(normalized_bucket_counts.get(10, 0)),
        hundreds=int(normalized_bucket_counts.get(100, 0)),
        xch_price_usd=xch_price_usd,
        bucket_counts_by_size=normalized_bucket_counts,
    )


def _evaluate_two_sided_market_actions(
    *,
    market: Any,
    counts_by_side: dict[str, dict[int, int]],
    xch_price_usd: float | None,
    now: datetime,
) -> list[PlannedAction]:
    actions: list[PlannedAction] = []
    for side in ("buy", "sell"):
        side_config = _strategy_config_for_side(market=market, side=side)
        side_state = _strategy_state_from_bucket_counts(
            counts_by_side.get(side, {}),
            xch_price_usd=xch_price_usd,
        )
        side_actions = evaluate_market(state=side_state, config=side_config, clock=now)
        actions.extend(
            PlannedAction(
                size=int(action.size),
                repeat=int(action.repeat),
                pair=action.pair,
                expiry_unit=action.expiry_unit,
                expiry_value=int(action.expiry_value),
                cancel_after_create=action.cancel_after_create,
                reason=action.reason,
                target_spread_bps=action.target_spread_bps,
                side=side,
            )
            for action in side_actions
        )
    return actions


def evaluate_reseed_candidates(
    *,
    strategy_config: StrategyConfig,
    xch_price_usd: float | None,
    clock: datetime,
) -> list[PlannedAction]:
    """Evaluate seed actions for offer rehydration (empty bucket state)."""
    return evaluate_market(
        state=_strategy_state_from_bucket_counts({}, xch_price_usd=xch_price_usd),
        config=strategy_config,
        clock=clock,
    )
