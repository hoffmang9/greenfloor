from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum

from greenfloor.core.cycle import apply_offer_signal_payload


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
    payload = apply_offer_signal_payload(state=state.value, signal=signal.value)
    return OfferTransition(
        old_state=OfferLifecycleState(str(payload["old_state"])),
        new_state=OfferLifecycleState(str(payload["new_state"])),
        signal=OfferSignal(str(payload["signal"])),
        action=str(payload["action"]),
        reason=str(payload["reason"]),
    )
