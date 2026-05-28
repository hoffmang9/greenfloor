"""Offer reconciliation transition kernel (Rust-backed)."""

from __future__ import annotations

from dataclasses import dataclass

from ._bridge import (
    resolve_missing_watched_offer_transition as _resolve_missing_watched_offer_transition,
    resolve_watched_offer_transition_from_signals as _resolve_watched_offer_transition_from_signals,
    unchanged_offer_transition as _unchanged_offer_transition,
    unsupported_venue_offer_transition as _unsupported_venue_offer_transition,
)

__all__ = [
    "CycleOfferTransition",
    "resolve_missing_watched_offer_transition",
    "resolve_watched_offer_transition_from_signals",
    "unchanged_offer_transition",
    "unsupported_venue_offer_transition",
]


@dataclass(frozen=True, slots=True)
class CycleOfferTransition:
    old_state: str
    new_state: str
    reason: str
    signal_source: str
    signal: str | None
    changed: bool
    immediate_requeue: bool
    taker_signal: str
    taker_diagnostic: str
    coinset_tx_ids: list[str]
    coinset_confirmed_tx_ids: list[str]
    coinset_mempool_tx_ids: list[str]


def resolve_missing_watched_offer_transition(*, current_state: str) -> CycleOfferTransition:
    return _require_cycle_offer_transition(
        _resolve_missing_watched_offer_transition(str(current_state))
    )


def resolve_watched_offer_transition_from_signals(
    *,
    current_state: str,
    status: int | None,
    coinset_tx_ids: list[str],
    coinset_confirmed_tx_ids: list[str],
    coinset_mempool_tx_ids: list[str],
) -> CycleOfferTransition:
    return _require_cycle_offer_transition(
        _resolve_watched_offer_transition_from_signals(
            current_state=str(current_state),
            status=status,
            coinset_tx_ids=coinset_tx_ids,
            coinset_confirmed_tx_ids=coinset_confirmed_tx_ids,
            coinset_mempool_tx_ids=coinset_mempool_tx_ids,
        )
    )


def unchanged_offer_transition(*, current_state: str, reason: str) -> CycleOfferTransition:
    return _require_cycle_offer_transition(
        _unchanged_offer_transition(str(current_state), str(reason))
    )


def unsupported_venue_offer_transition(*, current_state: str, venue: str) -> CycleOfferTransition:
    return _require_cycle_offer_transition(
        _unsupported_venue_offer_transition(str(current_state), str(venue))
    )


def _require_cycle_offer_transition(value: object) -> CycleOfferTransition:
    if not isinstance(value, CycleOfferTransition):
        raise TypeError("signer returned non-CycleOfferTransition result")
    return value
