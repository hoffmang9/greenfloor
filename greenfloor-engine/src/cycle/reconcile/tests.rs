use crate::cycle::lifecycle::OfferSignal;

use super::metadata::{
    REASON_COINSET_CONFIRMED, REASON_COINSET_MEMPOOL, REASON_COINSET_UNAVAILABLE,
    REASON_DEXIE_OFFER_NOT_FOUND, REASON_DEXIE_OFFER_NOT_FOUND_PRESERVED_TERMINAL,
    REASON_MISSING_STATUS, REASON_OK, SIGNAL_SOURCE_COINSET_MEMPOOL, SIGNAL_SOURCE_COINSET_WEBHOOK,
    SIGNAL_SOURCE_DEXIE_GET_OFFER_404, SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK, SIGNAL_SOURCE_NONE,
    TAKER_COINSET_TX_BLOCK_WEBHOOK, TAKER_DIAGNOSTIC_COINSET_CONFIRMED,
    TAKER_DIAGNOSTIC_COINSET_MEMPOOL, TAKER_DIAGNOSTIC_DEXIE_PATTERN_FALLBACK, TAKER_NONE,
};
use super::{
    decision::resolve_watched_offer_decision, resolve_missing_watched_offer_transition,
    resolve_watched_offer_transition_from_signals, unchanged_offer_transition,
    unsupported_venue_offer_transition, ReconcileState,
};

fn state(raw: &str) -> ReconcileState {
    ReconcileState::parse(raw).expect("valid reconcile state")
}

#[test]
fn coinset_confirmed_moves_open_offer_to_tx_block_confirmed() {
    let transition = resolve_watched_offer_transition_from_signals(
        "open",
        Some(0),
        vec!["c".repeat(64)],
        vec!["c".repeat(64)],
        vec![],
    )
    .expect("valid reconcile state");
    assert_eq!(transition.new_state, state("tx_block_confirmed"));
    assert_eq!(transition.reason, REASON_COINSET_CONFIRMED);
    assert_eq!(transition.signal_source, SIGNAL_SOURCE_COINSET_WEBHOOK);
    assert_eq!(transition.signal, Some(OfferSignal::TxConfirmed));
    assert_eq!(transition.taker_signal, TAKER_COINSET_TX_BLOCK_WEBHOOK);
    assert_eq!(
        transition.taker_diagnostic,
        TAKER_DIAGNOSTIC_COINSET_CONFIRMED
    );
}

#[test]
fn coinset_mempool_moves_open_offer_to_mempool_observed() {
    let transition = resolve_watched_offer_transition_from_signals(
        "open",
        Some(0),
        vec!["d".repeat(64)],
        vec![],
        vec!["d".repeat(64)],
    )
    .expect("valid reconcile state");
    assert_eq!(transition.new_state, state("mempool_observed"));
    assert_eq!(transition.reason, REASON_COINSET_MEMPOOL);
    assert_eq!(transition.signal_source, SIGNAL_SOURCE_COINSET_MEMPOOL);
    assert_eq!(
        transition.taker_diagnostic,
        TAKER_DIAGNOSTIC_COINSET_MEMPOOL
    );
}

#[test]
fn dexie_fallback_preserves_open_when_no_coinset_signal() {
    let transition = resolve_watched_offer_transition_from_signals(
        "open",
        Some(0),
        vec!["e".repeat(64)],
        vec![],
        vec![],
    )
    .expect("valid reconcile state");
    assert_eq!(transition.new_state, state("open"));
    assert_eq!(
        transition.signal_source,
        SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK
    );
    assert!(!transition.changed);
}

#[test]
fn missing_status_without_tx_ids() {
    let transition =
        resolve_watched_offer_transition_from_signals("open", None, vec![], vec![], vec![])
            .expect("valid reconcile state");
    assert_eq!(transition.new_state, state("open"));
    assert_eq!(transition.reason, REASON_MISSING_STATUS);
    assert_eq!(transition.signal_source, SIGNAL_SOURCE_NONE);
}

#[test]
fn coinset_signal_unavailable_for_offer() {
    let transition = resolve_watched_offer_transition_from_signals(
        "open",
        None,
        vec!["f".repeat(64)],
        vec![],
        vec![],
    )
    .expect("valid reconcile state");
    assert_eq!(transition.new_state, state("open"));
    assert_eq!(transition.reason, REASON_COINSET_UNAVAILABLE);
    assert_eq!(transition.signal_source, SIGNAL_SOURCE_NONE);
}

#[test]
fn dexie_status_fallback_when_no_coinset_tx_ids() {
    let transition =
        resolve_watched_offer_transition_from_signals("open", Some(4), vec![], vec![], vec![])
            .expect("valid reconcile state");
    assert_eq!(transition.new_state, state("tx_block_confirmed"));
    assert_eq!(
        transition.signal_source,
        SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK
    );
    assert_eq!(
        transition.taker_diagnostic,
        TAKER_DIAGNOSTIC_DEXIE_PATTERN_FALLBACK
    );
}

#[test]
fn dexie_cancelled_status_fallback() {
    let transition =
        resolve_watched_offer_transition_from_signals("open", Some(3), vec![], vec![], vec![])
            .expect("valid reconcile state");
    assert_eq!(transition.new_state, state("cancelled"));
    assert_eq!(
        transition.signal_source,
        SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK
    );
}

#[test]
fn missing_watched_offer_expires_open_offer() {
    let transition =
        resolve_missing_watched_offer_transition("open").expect("valid reconcile state");
    assert_eq!(transition.new_state, state("expired"));
    assert!(transition.changed);
    assert!(transition.immediate_requeue);
    assert_eq!(transition.signal, Some(OfferSignal::Expired));
    assert_eq!(transition.reason, REASON_DEXIE_OFFER_NOT_FOUND);
    assert_eq!(transition.signal_source, SIGNAL_SOURCE_DEXIE_GET_OFFER_404);
}

#[test]
fn missing_watched_offer_preserves_terminal_state() {
    let transition = resolve_missing_watched_offer_transition("tx_block_confirmed")
        .expect("valid reconcile state");
    assert_eq!(transition.new_state, state("tx_block_confirmed"));
    assert!(!transition.changed);
    assert_eq!(
        transition.reason,
        REASON_DEXIE_OFFER_NOT_FOUND_PRESERVED_TERMINAL
    );
}

#[test]
fn unchanged_offer_transition_factory() {
    let transition = unchanged_offer_transition("open", "dexie_lookup_error:boom")
        .expect("valid reconcile state");
    assert_eq!(transition.old_state, state("open"));
    assert_eq!(transition.new_state, state("open"));
    assert!(!transition.changed);
    assert_eq!(transition.taker_signal, TAKER_NONE);
}

#[test]
fn unsupported_venue_offer_transition_factory() {
    let transition =
        unsupported_venue_offer_transition("open", "splash").expect("valid reconcile state");
    assert_eq!(transition.new_state, state("reconcile_unsupported_venue"));
    assert!(transition.changed);
}

#[test]
fn unknown_reconcile_state_is_rejected() {
    let err = resolve_watched_offer_transition_from_signals(
        "not_a_real_state",
        None,
        vec![],
        vec![],
        vec![],
    )
    .expect_err("unknown state should fail");
    assert_eq!(
        err.to_string(),
        "unknown offer reconcile state: not_a_real_state"
    );
}

#[test]
fn decision_preserves_terminal_state_on_mempool_signal() {
    let current = state("tx_block_confirmed");
    let coinset_tx_ids = vec!["m".repeat(64)];
    let coinset_mempool_tx_ids = coinset_tx_ids.clone();
    let decision = resolve_watched_offer_decision(
        &current,
        Some(0),
        &coinset_tx_ids,
        &[],
        &coinset_mempool_tx_ids,
    );
    let transition = decision.into_cycle_transition(
        current.clone(),
        coinset_tx_ids,
        vec![],
        coinset_mempool_tx_ids,
    );
    assert_eq!(transition.new_state, state("tx_block_confirmed"));
    assert!(!transition.changed);
    assert_eq!(transition.reason, REASON_COINSET_MEMPOOL);
    assert!(transition.signal.is_none());
}

#[test]
fn decision_skips_coinset_confirmed_when_offer_is_cancelled() {
    let current = state("cancelled");
    let decision = resolve_watched_offer_decision(
        &current,
        Some(0),
        &["c".repeat(64)],
        &["c".repeat(64)],
        &[],
    );
    let transition = decision.into_cycle_transition_no_coinset(current);
    assert_eq!(transition.new_state, state("cancelled"));
    assert_eq!(transition.reason, REASON_OK);
    assert_eq!(
        transition.signal_source,
        SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK
    );
    assert!(!transition.changed);
}

#[test]
fn decision_skips_coinset_confirmed_when_dexie_status_is_cancelled() {
    let current = state("open");
    let decision = resolve_watched_offer_decision(
        &current,
        Some(3),
        &["c".repeat(64)],
        &["c".repeat(64)],
        &[],
    );
    let transition = decision.into_cycle_transition_no_coinset(current);
    assert_eq!(transition.new_state, state("cancelled"));
    assert_eq!(
        transition.signal_source,
        SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK
    );
}

#[test]
fn decision_confirmed_blocked_by_dexie_cancelled_falls_through_to_mempool() {
    let current = state("open");
    let decision = resolve_watched_offer_decision(
        &current,
        Some(3),
        &["c".repeat(64)],
        &["c".repeat(64)],
        &["m".repeat(64)],
    );
    let transition = decision.into_cycle_transition_no_coinset(current);
    assert_eq!(transition.new_state, state("mempool_observed"));
    assert_eq!(transition.reason, REASON_COINSET_MEMPOOL);
    assert_eq!(transition.signal, Some(OfferSignal::MempoolSeen));
}
