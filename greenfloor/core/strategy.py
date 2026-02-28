from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime


@dataclass(frozen=True, slots=True)
class MarketState:
    ones: int
    tens: int
    hundreds: int
    xch_price_usd: float | None = None


@dataclass(frozen=True, slots=True)
class StrategyConfig:
    pair: str
    ones_target: int = 5
    tens_target: int = 2
    hundreds_target: int = 1
    target_spread_bps: int | None = None
    min_xch_price_usd: float | None = None
    max_xch_price_usd: float | None = None
    offer_expiry_unit: str | None = None
    offer_expiry_value: int | None = None


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


_PAIR_EXPIRY_CONFIG: dict[str, tuple[str, int]] = {
    "xch": ("minutes", 10),
    "usdc": ("minutes", 10),
}


def evaluate_market(
    state: MarketState,
    config: StrategyConfig,
    clock: datetime,
) -> list[PlannedAction]:
    _ = clock
    pair = config.pair.lower()
    if pair == "xch":
        if state.xch_price_usd is None:
            return []
        if state.xch_price_usd <= 0:
            return []
        if config.min_xch_price_usd is not None and state.xch_price_usd < config.min_xch_price_usd:
            return []
        if config.max_xch_price_usd is not None and state.xch_price_usd > config.max_xch_price_usd:
            return []
    expiry_unit, expiry_value = _PAIR_EXPIRY_CONFIG.get(pair, _PAIR_EXPIRY_CONFIG["xch"])
    configured_expiry_unit = str(config.offer_expiry_unit or "").strip().lower()
    configured_expiry_value = (
        int(config.offer_expiry_value) if config.offer_expiry_value is not None else None
    )
    if configured_expiry_unit in {"minutes", "hours"} and configured_expiry_value is not None:
        if configured_expiry_value > 0:
            expiry_unit, expiry_value = configured_expiry_unit, configured_expiry_value

    offer_configs = [
        (1, state.ones, config.ones_target),
        (10, state.tens, config.tens_target),
        (100, state.hundreds, config.hundreds_target),
    ]

    actions: list[PlannedAction] = []
    for size, current, target in offer_configs:
        if current < target:
            actions.append(
                PlannedAction(
                    size=size,
                    repeat=target - current,
                    pair=pair,
                    expiry_unit=expiry_unit,
                    expiry_value=expiry_value,
                    cancel_after_create=True,
                    reason="below_target",
                    target_spread_bps=config.target_spread_bps,
                )
            )
    return actions
