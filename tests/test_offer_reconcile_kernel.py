from greenfloor.core.offer_reconcile import (
    CycleOfferTransition,
    resolve_missing_watched_offer_transition,
    resolve_watched_offer_transition,
)


def test_resolve_watched_offer_transition_coinset_confirmed() -> None:
    tx_id = "c" * 64
    transition = resolve_watched_offer_transition(
        current_state="open",
        status=0,
        coinset_tx_ids=[tx_id],
        coinset_confirmed_tx_ids=[tx_id],
        coinset_mempool_tx_ids=[],
    )
    assert isinstance(transition, CycleOfferTransition)
    assert transition.new_state == "tx_block_confirmed"
    assert transition.signal_source == "coinset_webhook"
    taker_signal, taker_diagnostic = transition.taker_fields(last_seen_status=0)
    assert taker_signal == "coinset_tx_block_webhook"
    assert taker_diagnostic == "coinset_tx_block_confirmed"


def test_resolve_missing_watched_offer_transition_expires_open() -> None:
    transition = resolve_missing_watched_offer_transition(current_state="open")
    assert transition.new_state == "expired"
    assert transition.immediate_requeue is True
