from greenfloor.core.offer_lifecycle import (
    OfferLifecycleState,
    OfferSignal,
    apply_offer_signal,
)


def test_open_to_mempool_observed() -> None:
    t = apply_offer_signal(OfferLifecycleState.OPEN, OfferSignal.MEMPOOL_SEEN)
    assert t.new_state == OfferLifecycleState.MEMPOOL_OBSERVED
    assert t.action == "mark_mempool_observed"


def test_mempool_to_confirmed() -> None:
    t = apply_offer_signal(OfferLifecycleState.MEMPOOL_OBSERVED, OfferSignal.TX_CONFIRMED)
    assert t.new_state == OfferLifecycleState.TX_BLOCK_CONFIRMED
    assert t.action == "reconcile_coins_and_offers"


def test_expiry_near_refresh_due() -> None:
    t = apply_offer_signal(OfferLifecycleState.OPEN, OfferSignal.EXPIRY_NEAR)
    assert t.new_state == OfferLifecycleState.REFRESH_DUE
    assert t.reason == "refresh_window_entered"
