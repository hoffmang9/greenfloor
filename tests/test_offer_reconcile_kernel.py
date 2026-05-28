from greenfloor.core.offer_reconcile import (
    CycleOfferTransition,
    resolve_missing_watched_offer_transition,
    resolve_watched_offer_transition_from_signals,
    unchanged_offer_transition,
    unsupported_venue_offer_transition,
)


def test_resolve_watched_offer_transition_coinset_confirmed() -> None:
    tx_id = "c" * 64
    transition = resolve_watched_offer_transition_from_signals(
        current_state="open",
        status=0,
        coinset_tx_ids=[tx_id],
        coinset_confirmed_tx_ids=[tx_id],
        coinset_mempool_tx_ids=[],
    )
    assert isinstance(transition, CycleOfferTransition)
    assert transition.new_state == "tx_block_confirmed"
    assert transition.reason == "coinset_tx_block_webhook_confirmed"
    assert transition.signal_source == "coinset_webhook"
    assert transition.signal == "tx_confirmed"
    assert transition.taker_signal == "coinset_tx_block_webhook"
    assert transition.taker_diagnostic == "coinset_tx_block_confirmed"


def test_resolve_watched_offer_transition_coinset_mempool() -> None:
    tx_id = "d" * 64
    transition = resolve_watched_offer_transition_from_signals(
        current_state="open",
        status=0,
        coinset_tx_ids=[tx_id],
        coinset_confirmed_tx_ids=[],
        coinset_mempool_tx_ids=[tx_id],
    )
    assert transition.new_state == "mempool_observed"
    assert transition.reason == "coinset_mempool_observed"
    assert transition.signal_source == "coinset_mempool"
    assert transition.taker_diagnostic == "coinset_mempool_observed"


def test_resolve_watched_offer_transition_preserves_open_without_coinset_signal() -> None:
    tx_id = "e" * 64
    transition = resolve_watched_offer_transition_from_signals(
        current_state="open",
        status=0,
        coinset_tx_ids=[tx_id],
        coinset_confirmed_tx_ids=[],
        coinset_mempool_tx_ids=[],
    )
    assert transition.new_state == "open"
    assert transition.signal_source == "dexie_status_fallback"
    assert not transition.changed
    assert transition.taker_diagnostic == "none"


def test_resolve_watched_offer_transition_missing_status_without_tx_ids() -> None:
    transition = resolve_watched_offer_transition_from_signals(
        current_state="open",
        status=None,
        coinset_tx_ids=[],
        coinset_confirmed_tx_ids=[],
        coinset_mempool_tx_ids=[],
    )
    assert transition.new_state == "open"
    assert transition.reason == "missing_status"
    assert transition.signal_source == "none"


def test_resolve_watched_offer_transition_coinset_signal_unavailable_for_offer() -> None:
    tx_id = "f" * 64
    transition = resolve_watched_offer_transition_from_signals(
        current_state="open",
        status=None,
        coinset_tx_ids=[tx_id],
        coinset_confirmed_tx_ids=[],
        coinset_mempool_tx_ids=[],
    )
    assert transition.new_state == "open"
    assert transition.reason == "coinset_signal_unavailable_for_offer"
    assert transition.signal_source == "none"


def test_resolve_watched_offer_transition_dexie_status_fallback() -> None:
    transition = resolve_watched_offer_transition_from_signals(
        current_state="open",
        status=4,
        coinset_tx_ids=[],
        coinset_confirmed_tx_ids=[],
        coinset_mempool_tx_ids=[],
    )
    assert transition.new_state == "tx_block_confirmed"
    assert transition.signal_source == "dexie_status_fallback"
    assert transition.taker_diagnostic == "dexie_status_pattern_fallback"


def test_resolve_watched_offer_transition_dexie_cancelled_fallback() -> None:
    transition = resolve_watched_offer_transition_from_signals(
        current_state="open",
        status=3,
        coinset_tx_ids=[],
        coinset_confirmed_tx_ids=[],
        coinset_mempool_tx_ids=[],
    )
    assert transition.new_state == "cancelled"
    assert transition.signal_source == "dexie_status_fallback"


def test_resolve_missing_watched_offer_transition_expires_open() -> None:
    transition = resolve_missing_watched_offer_transition(current_state="open")
    assert transition.new_state == "expired"
    assert transition.immediate_requeue is True
    assert transition.signal == "expired"


def test_resolve_missing_watched_offer_transition_preserves_terminal_state() -> None:
    transition = resolve_missing_watched_offer_transition(current_state="tx_block_confirmed")
    assert transition.new_state == "tx_block_confirmed"
    assert not transition.changed


def test_unchanged_offer_transition_factory() -> None:
    transition = unchanged_offer_transition(
        current_state="open",
        reason="dexie_lookup_error:boom",
    )
    assert transition.new_state == "open"
    assert not transition.changed
    assert transition.taker_signal == "none"


def test_unsupported_venue_offer_transition_factory() -> None:
    transition = unsupported_venue_offer_transition(current_state="open", venue="splash")
    assert transition.new_state == "reconcile_unsupported_venue"
    assert transition.changed
