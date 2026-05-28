"""Strategy evaluation input types (no bridge or planned-action imports)."""

from __future__ import annotations

from dataclasses import dataclass


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
