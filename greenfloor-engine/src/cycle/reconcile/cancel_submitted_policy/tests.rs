use std::collections::HashMap;

use chrono::TimeZone;

use super::*;
use crate::cycle::lifecycle::OfferSignal;
use crate::cycle::reconcile::metadata::{
    REASON_CANCEL_SUBMIT_STALE_ORPHAN, REASON_CANCEL_TX_CHAIN_CONFIRMED, REASON_COINSET_CONFIRMED,
    REASON_COINSET_MEMPOOL, REASON_COINSET_UNAVAILABLE, REASON_MISSING_STATUS, REASON_OK,
    SIGNAL_SOURCE_CANCEL_TX_CHAIN, SIGNAL_SOURCE_COINSET_MEMPOOL, SIGNAL_SOURCE_COINSET_WEBSOCKET,
    SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK, SIGNAL_SOURCE_NONE, TAKER_COINSET_TX_BLOCK_WEBSOCKET,
    TAKER_DIAGNOSTIC_CANCEL_TX_CHAIN_CONFIRMED, TAKER_DIAGNOSTIC_COINSET_CONFIRMED,
    TAKER_DIAGNOSTIC_COINSET_MEMPOOL, TAKER_NONE,
};
use crate::cycle::reconcile::ReconcileState;
use crate::storage::OfferStateListRow;

fn row(
    offer_id: &str,
    state: &str,
    cancel_tx_id: Option<&str>,
    updated_at: &str,
    cancel_submitted_at: Option<&str>,
) -> OfferStateListRow {
    OfferStateListRow {
        offer_id: offer_id.to_string(),
        market_id: "m1".to_string(),
        state: state.to_string(),
        last_seen_status: None,
        updated_at: updated_at.to_string(),
        cancel_submitted_tx_id: cancel_tx_id.map(str::to_string),
        cancel_submitted_at: cancel_submitted_at.map(str::to_string),
        publish_venue: None,
    }
}

fn coinset_id(label: char) -> String {
    label.to_string().repeat(64)
}

fn coinset_summary(
    tx_ids: &[String],
    confirmed: &[String],
    mempool: &[String],
) -> CoinsetSignalSummary {
    CoinsetSignalSummary::from_tx_lists(tx_ids, confirmed, mempool)
}

fn stale_cancel_ctx(now: chrono::DateTime<Utc>) -> CancelSubmittedContext {
    let anchor = (now - chrono::Duration::seconds(600)).to_rfc3339();
    CancelSubmittedContext {
        cancel_tx_id: Some(coinset_id('a')),
        cancel_tx_signal: Some(TxSignalStateRow {
            mempool_observed_at: Some(anchor.clone()),
            tx_block_confirmed_at: None,
        }),
        cancel_submitted_at: Some(anchor),
    }
}

#[test]
fn cancel_submit_in_flight_while_mempool_unconfirmed_within_grace() {
    let submitted = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let ctx = CancelSubmittedContext {
        cancel_tx_id: Some("tx1".to_string()),
        cancel_tx_signal: Some(TxSignalStateRow {
            mempool_observed_at: Some(submitted.to_rfc3339()),
            tx_block_confirmed_at: None,
        }),
        cancel_submitted_at: Some(submitted.to_rfc3339()),
    };
    assert!(is_cancel_submit_in_flight(
        &ctx,
        submitted + chrono::Duration::seconds(60),
        &[]
    ));
}

#[test]
fn defer_stops_when_cancel_tx_chain_confirmed_within_grace() {
    let submitted = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let ctx = CancelSubmittedContext {
        cancel_tx_id: Some("tx1".to_string()),
        cancel_tx_signal: Some(TxSignalStateRow {
            mempool_observed_at: Some(submitted.to_rfc3339()),
            tx_block_confirmed_at: Some((submitted + chrono::Duration::seconds(30)).to_rfc3339()),
        }),
        cancel_submitted_at: Some(submitted.to_rfc3339()),
    };
    let chain_confirmed = chain_confirmed_tx_ids_from_signals(&HashMap::from([(
        "tx1".to_string(),
        ctx.cancel_tx_signal.clone().expect("signal"),
    )]));
    assert!(!is_cancel_submit_in_flight(
        &ctx,
        submitted + chrono::Duration::seconds(60),
        &chain_confirmed,
    ));
}

#[test]
fn stale_reset_ineligible_when_cancel_tx_confirmed_but_dexie_still_open() {
    let ctx = CancelSubmittedContext {
        cancel_tx_id: Some("tx1".to_string()),
        cancel_tx_signal: Some(TxSignalStateRow {
            mempool_observed_at: Some("2020-01-01T00:00:00Z".to_string()),
            tx_block_confirmed_at: Some("2020-01-01T00:01:00Z".to_string()),
        }),
        cancel_submitted_at: None,
    };
    assert!(!cancel_submit_stale_reset_eligible(
        &ctx,
        Utc.with_ymd_and_hms(2020, 1, 1, 0, 2, 0).unwrap(),
        &[]
    ));
}

#[test]
fn grace_allows_orphan_cancel_shortly_after_submit() {
    let submitted = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let now = submitted + chrono::Duration::seconds(60);
    assert!(cancel_submit_within_grace_at(
        Some(&submitted.to_rfc3339()),
        now
    ));
    assert!(!cancel_submit_within_grace_at(
        Some(&submitted.to_rfc3339()),
        submitted + chrono::Duration::seconds(600)
    ));
}

#[test]
fn tracked_mempool_only_unconfirmed_unwedges_after_grace() {
    let submitted = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let after_grace = submitted + chrono::Duration::seconds(600);
    let ctx = CancelSubmittedContext {
        cancel_tx_id: Some("a".repeat(64)),
        cancel_tx_signal: Some(TxSignalStateRow {
            mempool_observed_at: Some(submitted.to_rfc3339()),
            tx_block_confirmed_at: None,
        }),
        cancel_submitted_at: Some(submitted.to_rfc3339()),
    };
    assert!(!is_cancel_submit_in_flight(&ctx, after_grace, &[]));
    assert!(cancel_submit_stale_reset_eligible(&ctx, after_grace, &[]));
    let transition = resolve_cancel_submitted_transition(
        Some(DEXIE_STATUS_OPEN),
        coinset_summary(&[], &[], &[]),
        &[],
        &ctx,
        after_grace,
    )
    .into_cycle_transition_no_coinset(ReconcileState::CancelSubmitted);
    assert_eq!(
        transition.new_state,
        ReconcileState::Lifecycle(OfferLifecycleState::Open)
    );
}

#[test]
fn stale_reset_blocked_when_cancel_tx_in_confirmed_list() {
    let submitted = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let after_grace = submitted + chrono::Duration::seconds(600);
    let cancel_tx_id = "a".repeat(64);
    let ctx = CancelSubmittedContext {
        cancel_tx_id: Some(cancel_tx_id.clone()),
        cancel_tx_signal: Some(TxSignalStateRow {
            mempool_observed_at: Some(submitted.to_rfc3339()),
            tx_block_confirmed_at: None,
        }),
        cancel_submitted_at: Some(submitted.to_rfc3339()),
    };
    assert!(!cancel_submit_stale_reset_eligible(
        &ctx,
        after_grace,
        std::slice::from_ref(&cancel_tx_id)
    ));
    let transition = resolve_cancel_submitted_transition(
        Some(DEXIE_STATUS_OPEN),
        coinset_summary(&[], &[], &[]),
        std::slice::from_ref(&cancel_tx_id),
        &ctx,
        after_grace,
    )
    .into_cycle_transition_no_coinset(ReconcileState::CancelSubmitted);
    assert_eq!(transition.new_state, ReconcileState::Cancelled);
}

#[test]
fn tracked_cancel_tx_id_allows_stale_reset_without_signal_after_grace() {
    let submitted = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let after_grace = submitted + chrono::Duration::seconds(600);
    let ctx = CancelSubmittedContext {
        cancel_tx_id: Some("a".repeat(64)),
        cancel_tx_signal: None,
        cancel_submitted_at: Some(submitted.to_rfc3339()),
    };
    assert!(!is_cancel_submit_in_flight(&ctx, after_grace, &[]));
    assert!(cancel_submit_stale_reset_eligible(&ctx, after_grace, &[]));
}

#[test]
fn stale_reset_still_allowed_without_recorded_cancel_tx_id() {
    let submitted = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let after_grace = submitted + chrono::Duration::seconds(600);
    let ctx = CancelSubmittedContext {
        cancel_tx_id: None,
        cancel_tx_signal: None,
        cancel_submitted_at: Some(submitted.to_rfc3339()),
    };
    assert!(!is_cancel_submit_in_flight(&ctx, after_grace, &[]));
    assert!(cancel_submit_stale_reset_eligible(&ctx, after_grace, &[]));
}

#[test]
fn stale_reset_uses_cancel_submitted_at_not_refreshed_updated_at() {
    let submitted = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let refreshed = submitted + chrono::Duration::seconds(240);
    let after_grace = submitted + chrono::Duration::seconds(600);
    let db_row = row(
        "offer-orphan",
        "cancel_submitted",
        None,
        &refreshed.to_rfc3339(),
        Some(&submitted.to_rfc3339()),
    );
    let ctx = CancelSubmittedContext::from_row_and_signals(&db_row, &HashMap::new());
    assert_eq!(
        ctx.cancel_submitted_at.as_deref(),
        Some(submitted.to_rfc3339().as_str())
    );
    assert!(cancel_submit_stale_reset_eligible(&ctx, after_grace, &[]));
}

#[test]
fn grace_anchor_ignores_refreshed_updated_at_without_cancel_submitted_at() {
    let submitted = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let refreshed = submitted + chrono::Duration::seconds(240);
    let tx_id = "a".repeat(64);
    let db_row = row(
        "offer-legacy",
        "cancel_submitted",
        Some(&tx_id),
        &refreshed.to_rfc3339(),
        None,
    );
    let mut signals = HashMap::new();
    signals.insert(
        tx_id.clone(),
        TxSignalStateRow {
            mempool_observed_at: Some(submitted.to_rfc3339()),
            tx_block_confirmed_at: None,
        },
    );
    let ctx = CancelSubmittedContext::from_row_and_signals(&db_row, &signals);
    assert_eq!(ctx.cancel_submitted_at, None);
    assert!(is_cancel_submit_in_flight(
        &ctx,
        submitted + chrono::Duration::seconds(60),
        &[]
    ));
    assert!(!is_cancel_submit_in_flight(
        &ctx,
        submitted + chrono::Duration::seconds(600),
        &[]
    ));
}

#[test]
fn cancel_tx_chain_confirmed_moves_to_cancelled() {
    let ctx = CancelSubmittedContext {
        cancel_tx_id: Some("tx1".to_string()),
        cancel_tx_signal: Some(TxSignalStateRow {
            mempool_observed_at: Some("2020-01-01T00:00:00Z".to_string()),
            tx_block_confirmed_at: Some("2020-01-01T00:01:00Z".to_string()),
        }),
        cancel_submitted_at: None,
    };
    let transition = resolve_cancel_submitted_transition(
        Some(DEXIE_STATUS_OPEN),
        coinset_summary(&[], &[], &[]),
        &[],
        &ctx,
        Utc.with_ymd_and_hms(2020, 1, 1, 0, 2, 0).unwrap(),
    )
    .into_cycle_transition_no_coinset(ReconcileState::CancelSubmitted);
    assert_eq!(transition.new_state, ReconcileState::Cancelled);
    assert_eq!(transition.reason, REASON_CANCEL_TX_CHAIN_CONFIRMED);
    assert_eq!(transition.signal_source, SIGNAL_SOURCE_CANCEL_TX_CHAIN);
    assert_eq!(
        transition.taker_diagnostic,
        TAKER_DIAGNOSTIC_CANCEL_TX_CHAIN_CONFIRMED
    );
}

#[test]
fn cancel_tx_chain_confirmed_beats_dexie_linked_taker_confirm() {
    let ctx = CancelSubmittedContext {
        cancel_tx_id: Some("tx1".to_string()),
        cancel_tx_signal: Some(TxSignalStateRow {
            mempool_observed_at: Some("2020-01-01T00:00:00Z".to_string()),
            tx_block_confirmed_at: Some("2020-01-01T00:01:00Z".to_string()),
        }),
        cancel_submitted_at: None,
    };
    let taker_tx = "b".repeat(64);
    let transition = resolve_cancel_submitted_transition(
        Some(DEXIE_STATUS_OPEN),
        coinset_summary(
            std::slice::from_ref(&taker_tx),
            std::slice::from_ref(&taker_tx),
            &[],
        ),
        &[],
        &ctx,
        Utc.with_ymd_and_hms(2020, 1, 1, 0, 2, 0).unwrap(),
    )
    .into_cycle_transition_no_coinset(ReconcileState::CancelSubmitted);
    assert_eq!(transition.new_state, ReconcileState::Cancelled);
}

#[test]
fn taker_confirmed_while_cancel_in_flight_promotes_to_tx_block_confirmed() {
    let cancel_tx = "a".repeat(64);
    let taker_tx = "b".repeat(64);
    let ctx = CancelSubmittedContext {
        cancel_tx_id: Some(cancel_tx),
        cancel_tx_signal: Some(TxSignalStateRow {
            mempool_observed_at: Some("2020-01-01T00:00:00Z".to_string()),
            tx_block_confirmed_at: None,
        }),
        cancel_submitted_at: Some("2020-01-01T00:00:00Z".to_string()),
    };
    let transition = resolve_cancel_submitted_transition(
        Some(DEXIE_STATUS_OPEN),
        coinset_summary(
            std::slice::from_ref(&taker_tx),
            std::slice::from_ref(&taker_tx),
            &[],
        ),
        &[],
        &ctx,
        Utc.with_ymd_and_hms(2020, 1, 1, 0, 2, 0).unwrap(),
    )
    .into_cycle_transition_no_coinset(ReconcileState::CancelSubmitted);
    assert_eq!(
        transition.new_state,
        ReconcileState::Lifecycle(OfferLifecycleState::TxBlockConfirmed)
    );
}

#[test]
fn cancel_tx_mempool_only_does_not_promote_to_mempool_observed() {
    let cancel_tx = "a".repeat(64);
    let submitted = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let ctx = CancelSubmittedContext {
        cancel_tx_id: Some(cancel_tx.clone()),
        cancel_tx_signal: Some(TxSignalStateRow {
            mempool_observed_at: Some(submitted.to_rfc3339()),
            tx_block_confirmed_at: None,
        }),
        cancel_submitted_at: Some(submitted.to_rfc3339()),
    };
    let transition = resolve_cancel_submitted_transition(
        Some(DEXIE_STATUS_OPEN),
        coinset_summary(&[], &[], &[]),
        &[],
        &ctx,
        submitted + chrono::Duration::seconds(60),
    )
    .into_cycle_transition_no_coinset(ReconcileState::CancelSubmitted);
    assert_eq!(transition.new_state, ReconcileState::CancelSubmitted);
    assert!(!transition.changed);
}

#[test]
fn cancel_submitted_moves_to_cancelled_on_dexie_status_3() {
    let now = Utc::now();
    let transition = resolve_cancel_submitted_transition(
        Some(DEXIE_STATUS_CANCELLED),
        coinset_summary(&[], &[], &[]),
        &[],
        &stale_cancel_ctx(now),
        now,
    )
    .into_cycle_transition_no_coinset(ReconcileState::CancelSubmitted);
    assert_eq!(transition.new_state, ReconcileState::Cancelled);
    assert_eq!(transition.reason, REASON_OK);
    assert_eq!(
        transition.signal_source,
        SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK
    );
    assert!(transition.changed);
}

#[test]
fn cancel_submitted_moves_to_mempool_observed_on_dexie_taker_mempool() {
    let now = Utc::now();
    let taker_tx = coinset_id('x');
    let transition = resolve_cancel_submitted_transition(
        Some(DEXIE_STATUS_OPEN),
        coinset_summary(
            std::slice::from_ref(&taker_tx),
            &[],
            std::slice::from_ref(&coinset_id('m')),
        ),
        &[],
        &stale_cancel_ctx(now),
        now,
    )
    .into_cycle_transition_no_coinset(ReconcileState::CancelSubmitted);
    assert_eq!(
        transition.new_state,
        ReconcileState::Lifecycle(OfferLifecycleState::MempoolObserved)
    );
    assert_eq!(transition.reason, REASON_COINSET_MEMPOOL);
    assert_eq!(transition.signal_source, SIGNAL_SOURCE_COINSET_MEMPOOL);
    assert_eq!(transition.signal, Some(OfferSignal::MempoolSeen));
    assert_eq!(
        transition.taker_diagnostic,
        TAKER_DIAGNOSTIC_COINSET_MEMPOOL
    );
}

#[test]
fn cancel_submitted_moves_to_tx_block_confirmed_on_dexie_taker_confirm() {
    let now = Utc::now();
    let taker_tx = coinset_id('x');
    let confirmed = coinset_id('c');
    let transition = resolve_cancel_submitted_transition(
        Some(DEXIE_STATUS_OPEN),
        coinset_summary(
            std::slice::from_ref(&taker_tx),
            std::slice::from_ref(&confirmed),
            &[],
        ),
        &[],
        &stale_cancel_ctx(now),
        now,
    )
    .into_cycle_transition_no_coinset(ReconcileState::CancelSubmitted);
    assert_eq!(
        transition.new_state,
        ReconcileState::Lifecycle(OfferLifecycleState::TxBlockConfirmed)
    );
    assert_eq!(transition.reason, REASON_COINSET_CONFIRMED);
    assert_eq!(transition.signal_source, SIGNAL_SOURCE_COINSET_WEBSOCKET);
    assert_eq!(transition.signal, Some(OfferSignal::TxConfirmed));
    assert_eq!(transition.taker_signal, TAKER_COINSET_TX_BLOCK_WEBSOCKET);
    assert_eq!(
        transition.taker_diagnostic,
        TAKER_DIAGNOSTIC_COINSET_CONFIRMED
    );
}

#[test]
fn cancel_submitted_dexie_open_resets_to_open_for_cancel_retry() {
    let now = Utc::now();
    let transition = resolve_cancel_submitted_transition(
        Some(DEXIE_STATUS_OPEN),
        coinset_summary(&[], &[], &[]),
        &[],
        &stale_cancel_ctx(now),
        now,
    )
    .into_cycle_transition_no_coinset(ReconcileState::CancelSubmitted);
    assert_eq!(
        transition.new_state,
        ReconcileState::Lifecycle(OfferLifecycleState::Open)
    );
    assert_eq!(transition.reason, REASON_CANCEL_SUBMIT_STALE_ORPHAN);
    assert_eq!(transition.signal_source, SIGNAL_SOURCE_NONE);
    assert_eq!(transition.taker_signal, TAKER_NONE);
}

#[test]
fn cancel_submitted_coinset_orphan_resets_to_open_after_grace() {
    let now = Utc::now();
    let transition = resolve_cancel_submitted_transition(
        None,
        coinset_summary(&[], &[], &[]),
        &[],
        &stale_cancel_ctx(now),
        now,
    )
    .into_cycle_transition_no_coinset(ReconcileState::CancelSubmitted);
    assert_eq!(
        transition.new_state,
        ReconcileState::Lifecycle(OfferLifecycleState::Open)
    );
    assert_eq!(transition.reason, REASON_CANCEL_SUBMIT_STALE_ORPHAN);
    assert_eq!(transition.signal_source, SIGNAL_SOURCE_NONE);
    assert_eq!(transition.taker_signal, TAKER_NONE);
}

#[test]
fn cancel_submitted_partial_coinset_activity_preserves_before_stale_reset() {
    let now = Utc::now();
    let partial_tx = coinset_id('p');
    let transition = resolve_cancel_submitted_transition(
        None,
        coinset_summary(std::slice::from_ref(&partial_tx), &[], &[]),
        &[],
        &stale_cancel_ctx(now),
        now,
    )
    .into_cycle_transition_no_coinset(ReconcileState::CancelSubmitted);
    assert_eq!(transition.new_state, ReconcileState::CancelSubmitted);
    assert_eq!(transition.reason, REASON_COINSET_UNAVAILABLE);
    assert!(!transition.changed);
}

#[test]
fn cancel_submitted_coinset_within_grace_stays_cancel_submitted() {
    let submitted = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let ctx = CancelSubmittedContext {
        cancel_tx_id: Some("tx1".to_string()),
        cancel_tx_signal: Some(TxSignalStateRow {
            mempool_observed_at: Some(submitted.to_rfc3339()),
            tx_block_confirmed_at: None,
        }),
        cancel_submitted_at: Some(submitted.to_rfc3339()),
    };
    let transition = resolve_cancel_submitted_transition(
        None,
        coinset_summary(&[], &[], &[]),
        &[],
        &ctx,
        submitted + chrono::Duration::seconds(60),
    )
    .into_cycle_transition_no_coinset(ReconcileState::CancelSubmitted);
    assert_eq!(transition.new_state, ReconcileState::CancelSubmitted);
    assert_eq!(transition.reason, REASON_MISSING_STATUS);
}

#[test]
fn allowed_cancel_target_offer_ids_defers_only_in_flight_cancel_submitted() {
    let now = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    let rows = vec![
        row("o1", "open", None, &now.to_rfc3339(), None),
        row(
            "o2",
            "cancel_submitted",
            Some("tx2"),
            &now.to_rfc3339(),
            Some(&now.to_rfc3339()),
        ),
    ];
    let mut signals = HashMap::new();
    signals.insert(
        "tx2".to_string(),
        TxSignalStateRow {
            mempool_observed_at: Some(now.to_rfc3339()),
            tx_block_confirmed_at: None,
        },
    );
    assert_eq!(
        allowed_cancel_target_offer_ids(
            &["o1".to_string(), "o2".to_string()],
            &rows,
            &signals,
            now,
        ),
        vec!["o1".to_string()]
    );
}
