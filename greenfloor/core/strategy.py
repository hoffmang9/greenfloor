from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime


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


_DEFAULT_OFFER_EXPIRY_MINUTES = 10


def _strategy_target_counts(config: StrategyConfig) -> list[tuple[int, int]]:
    if config.target_counts_by_size:
        return sorted(
            (
                (int(size), int(target))
                for size, target in config.target_counts_by_size.items()
                if int(size) > 0 and int(target) >= 0
            ),
            key=lambda entry: entry[0],
        )
    return [
        (1, int(config.ones_target)),
        (10, int(config.tens_target)),
        (100, int(config.hundreds_target)),
    ]


def _state_count_for_size(state: MarketState, size: int) -> int:
    if state.bucket_counts_by_size is not None:
        return int(state.bucket_counts_by_size.get(size, 0))
    if size == 1:
        return int(state.ones)
    if size == 10:
        return int(state.tens)
    if size == 100:
        return int(state.hundreds)
    return 0


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
    expiry_minutes = (
        int(config.offer_expiry_minutes)
        if config.offer_expiry_minutes is not None and int(config.offer_expiry_minutes) > 0
        else _DEFAULT_OFFER_EXPIRY_MINUTES
    )

    actions: list[PlannedAction] = []
    for size, target in _strategy_target_counts(config):
        current = _state_count_for_size(state, size)
        if current < target:
            actions.append(
                PlannedAction(
                    size=size,
                    repeat=target - current,
                    side="sell",
                    pair=pair,
                    expiry_unit="minutes",
                    expiry_value=expiry_minutes,
                    cancel_after_create=True,
                    reason="below_target",
                    target_spread_bps=config.target_spread_bps,
                )
            )
    return actions
