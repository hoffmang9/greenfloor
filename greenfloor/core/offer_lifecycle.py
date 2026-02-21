from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum


class OfferLifecycleState(StrEnum):
    OPEN = "open"
    MEMPOOL_OBSERVED = "mempool_observed"
    TX_BLOCK_CONFIRMED = "tx_block_confirmed"
    REFRESH_DUE = "refresh_due"
    EXPIRED = "expired"


class OfferSignal(StrEnum):
    MEMPOOL_SEEN = "mempool_seen"
    TX_CONFIRMED = "tx_confirmed"
    EXPIRY_NEAR = "expiry_near"
    EXPIRED = "expired"
    REFRESH_POSTED = "refresh_posted"


@dataclass(frozen=True, slots=True)
class OfferTransition:
    old_state: OfferLifecycleState
    new_state: OfferLifecycleState
    signal: OfferSignal
    action: str
    reason: str


def apply_offer_signal(
    state: OfferLifecycleState,
    signal: OfferSignal,
) -> OfferTransition:
    if signal == OfferSignal.MEMPOOL_SEEN and state == OfferLifecycleState.OPEN:
        return OfferTransition(
            old_state=state,
            new_state=OfferLifecycleState.MEMPOOL_OBSERVED,
            signal=signal,
            action="mark_mempool_observed",
            reason="potential_take_seen",
        )
    if signal == OfferSignal.TX_CONFIRMED and state in {
        OfferLifecycleState.OPEN,
        OfferLifecycleState.MEMPOOL_OBSERVED,
    }:
        return OfferTransition(
            old_state=state,
            new_state=OfferLifecycleState.TX_BLOCK_CONFIRMED,
            signal=signal,
            action="reconcile_coins_and_offers",
            reason="take_confirmed_on_tx_block",
        )
    if signal == OfferSignal.EXPIRY_NEAR and state == OfferLifecycleState.OPEN:
        return OfferTransition(
            old_state=state,
            new_state=OfferLifecycleState.REFRESH_DUE,
            signal=signal,
            action="refresh_offer",
            reason="refresh_window_entered",
        )
    if signal == OfferSignal.REFRESH_POSTED and state == OfferLifecycleState.REFRESH_DUE:
        return OfferTransition(
            old_state=state,
            new_state=OfferLifecycleState.OPEN,
            signal=signal,
            action="track_new_offer_open",
            reason="offer_refreshed",
        )
    if signal == OfferSignal.EXPIRED and state in {
        OfferLifecycleState.OPEN,
        OfferLifecycleState.REFRESH_DUE,
    }:
        return OfferTransition(
            old_state=state,
            new_state=OfferLifecycleState.EXPIRED,
            signal=signal,
            action="cleanup_offer_state",
            reason="offer_expired",
        )

    return OfferTransition(
        old_state=state,
        new_state=state,
        signal=signal,
        action="noop",
        reason="signal_ignored_for_state",
    )
