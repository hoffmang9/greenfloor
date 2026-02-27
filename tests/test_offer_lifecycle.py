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


def test_open_to_confirmed() -> None:
    t = apply_offer_signal(OfferLifecycleState.OPEN, OfferSignal.TX_CONFIRMED)
    assert t.new_state == OfferLifecycleState.TX_BLOCK_CONFIRMED
    assert t.action == "reconcile_coins_and_offers"


def test_expiry_near_refresh_due() -> None:
    t = apply_offer_signal(OfferLifecycleState.OPEN, OfferSignal.EXPIRY_NEAR)
    assert t.new_state == OfferLifecycleState.REFRESH_DUE
    assert t.reason == "refresh_window_entered"


def test_refresh_posted_returns_to_open() -> None:
    t = apply_offer_signal(OfferLifecycleState.REFRESH_DUE, OfferSignal.REFRESH_POSTED)
    assert t.new_state == OfferLifecycleState.OPEN
    assert t.action == "track_new_offer_open"
    assert t.reason == "offer_refreshed"


def test_expired_from_open() -> None:
    t = apply_offer_signal(OfferLifecycleState.OPEN, OfferSignal.EXPIRED)
    assert t.new_state == OfferLifecycleState.EXPIRED
    assert t.action == "cleanup_offer_state"


def test_expired_from_refresh_due() -> None:
    t = apply_offer_signal(OfferLifecycleState.REFRESH_DUE, OfferSignal.EXPIRED)
    assert t.new_state == OfferLifecycleState.EXPIRED
    assert t.action == "cleanup_offer_state"


def test_noop_for_irrelevant_signal() -> None:
    t = apply_offer_signal(OfferLifecycleState.TX_BLOCK_CONFIRMED, OfferSignal.MEMPOOL_SEEN)
    assert t.new_state == OfferLifecycleState.TX_BLOCK_CONFIRMED
    assert t.action == "noop"
    assert t.reason == "signal_ignored_for_state"


def test_noop_expired_state_ignores_further_expiry() -> None:
    t = apply_offer_signal(OfferLifecycleState.EXPIRED, OfferSignal.EXPIRED)
    assert t.new_state == OfferLifecycleState.EXPIRED
    assert t.action == "noop"


def test_noop_mempool_observed_ignores_expiry_near() -> None:
    t = apply_offer_signal(OfferLifecycleState.MEMPOOL_OBSERVED, OfferSignal.EXPIRY_NEAR)
    assert t.new_state == OfferLifecycleState.MEMPOOL_OBSERVED
    assert t.action == "noop"


def test_transition_preserves_old_state() -> None:
    t = apply_offer_signal(OfferLifecycleState.OPEN, OfferSignal.MEMPOOL_SEEN)
    assert t.old_state == OfferLifecycleState.OPEN
    assert t.signal == OfferSignal.MEMPOOL_SEEN
