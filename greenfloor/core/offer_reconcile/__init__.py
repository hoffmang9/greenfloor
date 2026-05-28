"""Offer reconciliation transition kernel (Rust-backed)."""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from typing import TypeVar

from greenfloor.core.cycle._bridge import _import_signer

__all__ = [
    "CycleOfferTransition",
    "resolve_missing_watched_offer_transition",
    "resolve_watched_offer_transition_from_signals",
    "unchanged_offer_transition",
    "unsupported_venue_offer_transition",
]

_CallableT = TypeVar("_CallableT", bound=Callable[..., object])


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


def _require_cycle_offer_transition(value: object) -> CycleOfferTransition:
    if not isinstance(value, CycleOfferTransition):
        raise TypeError("signer returned non-CycleOfferTransition result")
    return value


def _typed_transition(call: _CallableT, /, *args: object, **kwargs: object) -> CycleOfferTransition:
    return _require_cycle_offer_transition(call(*args, **kwargs))


def resolve_missing_watched_offer_transition(*, current_state: str) -> CycleOfferTransition:
    return _typed_transition(
        _import_signer().resolve_missing_watched_offer_transition,
        str(current_state),
    )


def resolve_watched_offer_transition_from_signals(
    *,
    current_state: str,
    status: int | None,
    coinset_tx_ids: list[str],
    coinset_confirmed_tx_ids: list[str],
    coinset_mempool_tx_ids: list[str],
) -> CycleOfferTransition:
    return _typed_transition(
        _import_signer().resolve_watched_offer_transition_from_signals,
        str(current_state),
        status,
        list(coinset_tx_ids),
        list(coinset_confirmed_tx_ids),
        list(coinset_mempool_tx_ids),
    )


def unchanged_offer_transition(*, current_state: str, reason: str) -> CycleOfferTransition:
    return _typed_transition(
        _import_signer().unchanged_offer_transition,
        str(current_state),
        str(reason),
    )


def unsupported_venue_offer_transition(*, current_state: str, venue: str) -> CycleOfferTransition:
    return _typed_transition(
        _import_signer().unsupported_venue_offer_transition,
        str(current_state),
        str(venue),
    )
