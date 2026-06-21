use super::{
    decision::resolve_watched_offer_decision, resolve_missing_watched_offer_transition,
    resolve_watched_offer_transition_from_signals, state::ReconcileState,
    unchanged_offer_transition, unsupported_venue_offer_transition,
};

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
    assert_eq!(transition.new_state, "tx_block_confirmed");
    assert_eq!(transition.reason, "coinset_tx_block_webhook_confirmed");
    assert_eq!(transition.signal_source, "coinset_webhook");
    assert_eq!(transition.signal.as_deref(), Some("tx_confirmed"));
    assert_eq!(transition.taker_signal, "coinset_tx_block_webhook");
    assert_eq!(transition.taker_diagnostic, "coinset_tx_block_confirmed");
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
    assert_eq!(transition.new_state, "mempool_observed");
    assert_eq!(transition.reason, "coinset_mempool_observed");
    assert_eq!(transition.signal_source, "coinset_mempool");
    assert_eq!(transition.taker_diagnostic, "coinset_mempool_observed");
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
    assert_eq!(transition.new_state, "open");
    assert_eq!(transition.signal_source, "dexie_status_fallback");
    assert!(!transition.changed);
}

#[test]
fn missing_status_without_tx_ids() {
    let transition =
        resolve_watched_offer_transition_from_signals("open", None, vec![], vec![], vec![])
            .expect("valid reconcile state");
    assert_eq!(transition.new_state, "open");
    assert_eq!(transition.reason, "missing_status");
    assert_eq!(transition.signal_source, "none");
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
    assert_eq!(transition.new_state, "open");
    assert_eq!(transition.reason, "coinset_signal_unavailable_for_offer");
    assert_eq!(transition.signal_source, "none");
}

#[test]
fn dexie_status_fallback_when_no_coinset_tx_ids() {
    let transition =
        resolve_watched_offer_transition_from_signals("open", Some(4), vec![], vec![], vec![])
            .expect("valid reconcile state");
    assert_eq!(transition.new_state, "tx_block_confirmed");
    assert_eq!(transition.signal_source, "dexie_status_fallback");
    assert_eq!(transition.taker_diagnostic, "dexie_status_pattern_fallback");
}

#[test]
fn dexie_cancelled_status_fallback() {
    let transition =
        resolve_watched_offer_transition_from_signals("open", Some(3), vec![], vec![], vec![])
            .expect("valid reconcile state");
    assert_eq!(transition.new_state, "cancelled");
    assert_eq!(transition.signal_source, "dexie_status_fallback");
}

#[test]
fn missing_watched_offer_expires_open_offer() {
    let transition =
        resolve_missing_watched_offer_transition("open").expect("valid reconcile state");
    assert_eq!(transition.new_state, "expired");
    assert!(transition.changed);
    assert!(transition.immediate_requeue);
    assert_eq!(transition.signal.as_deref(), Some("expired"));
}

#[test]
fn missing_watched_offer_preserves_terminal_state() {
    let transition = resolve_missing_watched_offer_transition("tx_block_confirmed")
        .expect("valid reconcile state");
    assert_eq!(transition.new_state, "tx_block_confirmed");
    assert!(!transition.changed);
}

#[test]
fn unchanged_offer_transition_factory() {
    let transition = unchanged_offer_transition("open", "dexie_lookup_error:boom")
        .expect("valid reconcile state");
    assert_eq!(transition.old_state, "open");
    assert_eq!(transition.new_state, "open");
    assert!(!transition.changed);
    assert_eq!(transition.taker_signal, "none");
}

#[test]
fn unsupported_venue_offer_transition_factory() {
    let transition =
        unsupported_venue_offer_transition("open", "splash").expect("valid reconcile state");
    assert_eq!(transition.new_state, "reconcile_unsupported_venue");
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
    let current = ReconcileState::parse("tx_block_confirmed").expect("valid reconcile state");
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
        "tx_block_confirmed",
        coinset_tx_ids,
        vec![],
        coinset_mempool_tx_ids,
    );
    assert_eq!(transition.new_state, "tx_block_confirmed");
    assert!(!transition.changed);
    assert_eq!(transition.reason, "coinset_mempool_observed");
    assert!(transition.signal.is_none());
}

#[test]
fn decision_skips_coinset_confirmed_when_offer_is_cancelled() {
    let current = ReconcileState::parse("cancelled").expect("valid reconcile state");
    let decision = resolve_watched_offer_decision(
        &current,
        Some(0),
        &["c".repeat(64)],
        &["c".repeat(64)],
        &[],
    );
    let transition = decision.into_cycle_transition_no_coinset("cancelled");
    assert_eq!(transition.new_state, "cancelled");
    assert_eq!(transition.reason, "ok");
    assert_eq!(transition.signal_source, "dexie_status_fallback");
    assert!(!transition.changed);
}

#[test]
fn decision_skips_coinset_confirmed_when_dexie_status_is_cancelled() {
    let current = ReconcileState::parse("open").expect("valid reconcile state");
    let decision = resolve_watched_offer_decision(
        &current,
        Some(3),
        &["c".repeat(64)],
        &["c".repeat(64)],
        &[],
    );
    let transition = decision.into_cycle_transition_no_coinset("open");
    assert_eq!(transition.new_state, "cancelled");
    assert_eq!(transition.signal_source, "dexie_status_fallback");
}
