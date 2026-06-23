//! Unified reconcile and daemon targeting policy for `cancel_submitted` offers.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::cycle::lifecycle::OfferLifecycleState;
use crate::hex::canonical_tx_id;
use crate::offer::dexie_payload::{DEXIE_STATUS_CANCELLED, DEXIE_STATUS_OPEN};
use crate::storage::{OfferStateListRow, TxSignalStateRow};

use super::builders::{
    cancel_tx_chain_confirmed_transition, preserve_state, transition_from_dexie_status,
};
use super::coinset_signals::CoinsetSignalSummary;
use super::dispatch::apply_coinset_taker_dispatch_if_present;
use super::metadata::{
    REASON_CANCEL_SUBMIT_STALE_DEXIE_OPEN, REASON_COINSET_UNAVAILABLE, REASON_MISSING_STATUS,
    SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK, TAKER_NONE,
};
use super::state::ReconcileState;
use super::transition::ReconcileTransition;

/// Grace period before treating orphan `cancel_submitted` (no recorded tx id) as stale.
pub(crate) const CANCEL_SUBMIT_TRACKING_GRACE_SECS: i64 = 5 * 60;

#[derive(Debug, Clone, Default)]
pub struct CancelSubmittedContext {
    pub cancel_tx_id: Option<String>,
    pub cancel_tx_signal: Option<TxSignalStateRow>,
    pub submitted_at: Option<String>,
}

impl CancelSubmittedContext {
    #[must_use]
    pub fn from_row_and_signals(
        row: &OfferStateListRow,
        signals: &HashMap<String, TxSignalStateRow>,
    ) -> Self {
        Self {
            cancel_tx_id: row.cancel_submitted_tx_id.clone(),
            cancel_tx_signal: row
                .cancel_submitted_tx_id
                .as_deref()
                .and_then(|tx_id| canonical_tx_id(tx_id).and_then(|id| signals.get(&id)))
                .cloned(),
            submitted_at: Some(row.updated_at.clone()),
        }
    }
}

/// Whether daemon cancel policy should skip targeting this offer for now.
#[must_use]
pub fn defer_cancel_target(ctx: &CancelSubmittedContext, now: DateTime<Utc>) -> bool {
    cancel_tx_in_flight(ctx, now)
}

/// Drop Dexie-open ids whose cancel submit is still in flight.
#[must_use]
pub fn filter_defer_cancel_submitted_targets(
    offer_ids: &[String],
    db_rows: &[OfferStateListRow],
    tx_signals: &HashMap<String, TxSignalStateRow>,
    now: DateTime<Utc>,
) -> Vec<String> {
    let defer_targets: std::collections::HashSet<&str> = db_rows
        .iter()
        .filter_map(|row| {
            if !ReconcileState::parse(&row.state).is_ok_and(|state| state.is_cancel_submitted()) {
                return None;
            }
            let ctx = CancelSubmittedContext::from_row_and_signals(row, tx_signals);
            defer_cancel_target(&ctx, now).then_some(row.offer_id.as_str())
        })
        .collect();
    offer_ids
        .iter()
        .filter(|offer_id| !defer_targets.contains(offer_id.as_str()))
        .cloned()
        .collect()
}

/// Resolve reconcile transition for an offer already in `cancel_submitted`.
pub(crate) fn resolve_cancel_submitted_transition(
    dexie_status: Option<i64>,
    coinset: CoinsetSignalSummary,
    ctx: &CancelSubmittedContext,
    now: DateTime<Utc>,
) -> ReconcileTransition {
    let current = ReconcileState::CancelSubmitted;
    if cancel_tx_confirmed(ctx) {
        return cancel_tx_chain_confirmed_transition();
    }
    if dexie_status == Some(DEXIE_STATUS_CANCELLED) {
        return transition_from_dexie_status(DEXIE_STATUS_CANCELLED, current);
    }
    if let Some(taker) = apply_coinset_taker_dispatch_if_present(coinset, dexie_status, &current) {
        return taker;
    }
    cancel_submitted_dexie_status_transition(dexie_status, coinset, ctx, now)
}

fn cancel_submitted_dexie_status_transition(
    dexie_status: Option<i64>,
    coinset: CoinsetSignalSummary,
    ctx: &CancelSubmittedContext,
    now: DateTime<Utc>,
) -> ReconcileTransition {
    match dexie_status {
        None if coinset.has_tx_ids => {
            preserve_state(&ReconcileState::CancelSubmitted, REASON_COINSET_UNAVAILABLE)
        }
        None => preserve_state(&ReconcileState::CancelSubmitted, REASON_MISSING_STATUS),
        Some(DEXIE_STATUS_OPEN) if stale_cancel_submit_eligible(ctx, now) => {
            ReconcileTransition::new(
                ReconcileState::Lifecycle(OfferLifecycleState::Open),
                REASON_CANCEL_SUBMIT_STALE_DEXIE_OPEN,
                SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK,
                None,
                TAKER_NONE,
                TAKER_NONE,
            )
        }
        Some(status) => transition_from_dexie_status(status, ReconcileState::CancelSubmitted),
    }
}

#[must_use]
fn cancel_tx_confirmed(ctx: &CancelSubmittedContext) -> bool {
    ctx.cancel_tx_signal
        .as_ref()
        .is_some_and(|signal| signal.tx_block_confirmed_at.is_some())
}

#[must_use]
fn tracked_cancel_tx_pending(ctx: &CancelSubmittedContext) -> bool {
    let Some(cancel_tx_id) = ctx
        .cancel_tx_id
        .as_deref()
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    let _ = cancel_tx_id;
    ctx.cancel_tx_signal
        .as_ref()
        .is_none_or(|signal| signal.tx_block_confirmed_at.is_none())
}

#[must_use]
fn orphan_cancel_submit_within_grace(submitted_at: Option<&str>, now: DateTime<Utc>) -> bool {
    let Some(raw) = submitted_at
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    let Ok(parsed) = DateTime::parse_from_rfc3339(raw) else {
        return false;
    };
    (now - parsed.with_timezone(&Utc)).num_seconds() < CANCEL_SUBMIT_TRACKING_GRACE_SECS
}

#[must_use]
fn cancel_tx_in_flight(ctx: &CancelSubmittedContext, now: DateTime<Utc>) -> bool {
    tracked_cancel_tx_pending(ctx)
        || orphan_cancel_submit_within_grace(ctx.submitted_at.as_deref(), now)
}

#[must_use]
fn stale_cancel_submit_eligible(ctx: &CancelSubmittedContext, now: DateTime<Utc>) -> bool {
    !cancel_tx_in_flight(ctx, now) && !cancel_tx_confirmed(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cycle::reconcile::metadata::{
        REASON_CANCEL_TX_CHAIN_CONFIRMED, SIGNAL_SOURCE_CANCEL_TX_CHAIN,
        TAKER_DIAGNOSTIC_CANCEL_TX_CHAIN_CONFIRMED,
    };
    use crate::storage::OfferStateListRow;
    use chrono::TimeZone;

    fn row(
        offer_id: &str,
        state: &str,
        cancel_tx_id: Option<&str>,
        updated_at: &str,
    ) -> OfferStateListRow {
        OfferStateListRow {
            offer_id: offer_id.to_string(),
            market_id: "m1".to_string(),
            state: state.to_string(),
            last_seen_status: None,
            updated_at: updated_at.to_string(),
            cancel_submitted_tx_id: cancel_tx_id.map(str::to_string),
        }
    }

    #[test]
    fn defer_cancel_target_while_mempool_unconfirmed() {
        let ctx = CancelSubmittedContext {
            cancel_tx_id: Some("tx1".to_string()),
            cancel_tx_signal: Some(TxSignalStateRow {
                mempool_observed_at: Some("2020-01-01T00:00:00Z".to_string()),
                tx_block_confirmed_at: None,
            }),
            submitted_at: None,
        };
        assert!(defer_cancel_target(
            &ctx,
            Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap()
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
            submitted_at: None,
        };
        assert!(!stale_cancel_submit_eligible(
            &ctx,
            Utc.with_ymd_and_hms(2020, 1, 1, 0, 2, 0).unwrap()
        ));
    }

    #[test]
    fn grace_allows_orphan_cancel_shortly_after_submit() {
        let submitted = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
        let now = submitted + chrono::Duration::seconds(60);
        assert!(orphan_cancel_submit_within_grace(
            Some(&submitted.to_rfc3339()),
            now
        ));
        assert!(!orphan_cancel_submit_within_grace(
            Some(&submitted.to_rfc3339()),
            submitted + chrono::Duration::seconds(600)
        ));
    }

    #[test]
    fn tracked_cancel_tx_id_stays_in_flight_without_signal_after_grace() {
        let submitted = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
        let after_grace = submitted + chrono::Duration::seconds(600);
        let ctx = CancelSubmittedContext {
            cancel_tx_id: Some("a".repeat(64)),
            cancel_tx_signal: None,
            submitted_at: Some(submitted.to_rfc3339()),
        };
        assert!(tracked_cancel_tx_pending(&ctx));
        assert!(cancel_tx_in_flight(&ctx, after_grace));
        assert!(!stale_cancel_submit_eligible(&ctx, after_grace));
    }

    #[test]
    fn stale_reset_still_allowed_without_recorded_cancel_tx_id() {
        let submitted = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
        let after_grace = submitted + chrono::Duration::seconds(600);
        let ctx = CancelSubmittedContext {
            cancel_tx_id: None,
            cancel_tx_signal: None,
            submitted_at: Some(submitted.to_rfc3339()),
        };
        assert!(!tracked_cancel_tx_pending(&ctx));
        assert!(!cancel_tx_in_flight(&ctx, after_grace));
        assert!(stale_cancel_submit_eligible(&ctx, after_grace));
    }

    #[test]
    fn cancel_tx_chain_confirmed_moves_to_cancelled() {
        let ctx = CancelSubmittedContext {
            cancel_tx_id: Some("tx1".to_string()),
            cancel_tx_signal: Some(TxSignalStateRow {
                mempool_observed_at: Some("2020-01-01T00:00:00Z".to_string()),
                tx_block_confirmed_at: Some("2020-01-01T00:01:00Z".to_string()),
            }),
            submitted_at: None,
        };
        let transition = resolve_cancel_submitted_transition(
            Some(DEXIE_STATUS_OPEN),
            CoinsetSignalSummary::default(),
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
            submitted_at: None,
        };
        let transition = resolve_cancel_submitted_transition(
            Some(DEXIE_STATUS_OPEN),
            CoinsetSignalSummary {
                has_tx_ids: true,
                has_confirmed: true,
                has_mempool: false,
            },
            &ctx,
            Utc.with_ymd_and_hms(2020, 1, 1, 0, 2, 0).unwrap(),
        )
        .into_cycle_transition_no_coinset(ReconcileState::CancelSubmitted);
        assert_eq!(transition.new_state, ReconcileState::Cancelled);
    }

    #[test]
    fn taker_confirmed_while_cancel_in_flight_promotes_to_tx_block_confirmed() {
        let ctx = CancelSubmittedContext {
            cancel_tx_id: Some("a".repeat(64)),
            cancel_tx_signal: Some(TxSignalStateRow {
                mempool_observed_at: Some("2020-01-01T00:00:00Z".to_string()),
                tx_block_confirmed_at: None,
            }),
            submitted_at: Some("2020-01-01T00:00:00Z".to_string()),
        };
        let transition = resolve_cancel_submitted_transition(
            Some(DEXIE_STATUS_OPEN),
            CoinsetSignalSummary {
                has_tx_ids: true,
                has_confirmed: true,
                has_mempool: false,
            },
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
    fn filter_defers_only_in_flight_cancel_submitted() {
        let now = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
        let rows = vec![
            row("o1", "open", None, &now.to_rfc3339()),
            row("o2", "cancel_submitted", Some("tx2"), &now.to_rfc3339()),
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
            filter_defer_cancel_submitted_targets(
                &["o1".to_string(), "o2".to_string()],
                &rows,
                &signals,
                now,
            ),
            vec!["o1".to_string()]
        );
    }
}
