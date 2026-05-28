"""Orchestration FFI types (market batch, stale sweep, parallel outcomes)."""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass(frozen=True, slots=True)
class ParallelActionOutcome:
    status: str
    transient_upstream: bool = False


@dataclass(frozen=True, slots=True)
class MarketBatchSelection:
    selected_market_ids: list[str]
    consumed_immediate_requeues: list[str]
    cursor: int
    immediate_requeue_ids: list[str]


@dataclass(frozen=True, slots=True)
class OfferStateRow:
    market_id: str
    offer_id: str
    state: str


@dataclass(frozen=True, slots=True)
class StaleSweepCandidate:
    market_id: str
    offer_id: str


@dataclass(frozen=True, slots=True)
class StaleSweepHit:
    market_id: str
    offer_id: str
    reason: str


@dataclass(frozen=True, slots=True)
class StaleSweepProgress:
    checked_offer_count: int = 0
    requeue_market_ids: list[str] = field(default_factory=list)
    hits: list[StaleSweepHit] = field(default_factory=list)
    truncated: bool = False
