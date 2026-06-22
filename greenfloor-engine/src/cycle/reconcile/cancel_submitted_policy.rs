//! Unified reconcile and daemon targeting policy for `cancel_submitted` offers.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::cycle::lifecycle::{OfferLifecycleState, OfferSignal};
use crate::offer::dexie_payload::{
    is_dexie_pattern_fallback_status, reconcile_from_dexie_status, DexieStatusReconcile,
    DEXIE_STATUS_CANCELLED, DEXIE_STATUS_OPEN,
};
use crate::storage::{OfferStateListRow, TxSignalStateRow};

use super::builders::{dexie_fallback_transition, open_signal_transition, preserve_state};
use super::metadata::{
    REASON_CANCEL_SUBMIT_STALE_DEXIE_OPEN, REASON_COINSET_CONFIRMED, REASON_COINSET_MEMPOOL,
    REASON_COINSET_UNAVAILABLE, REASON_MISSING_STATUS, SIGNAL_SOURCE_COINSET_MEMPOOL,
    SIGNAL_SOURCE_COINSET_WEBHOOK, SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK,
    TAKER_COINSET_TX_BLOCK_WEBHOOK, TAKER_DIAGNOSTIC_COINSET_CONFIRMED,
    TAKER_DIAGNOSTIC_COINSET_MEMPOOL, TAKER_DIAGNOSTIC_DEXIE_PATTERN_FALLBACK, TAKER_NONE,
};
use super::state::ReconcileState;
use super::transition::ReconcileTransition;

/// Grace period after submit before treating an untracked cancel tx as stale.
pub(crate) const CANCEL_SUBMIT_TRACKING_GRACE_SECS: i64 = 5 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoinsetOfferSignals {
    pub has_tx_ids: bool,
    pub has_confirmed: bool,
    pub has_mempool: bool,
}

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
        let cancel_tx_id = row.cancel_submitted_tx_id.clone();
        let cancel_tx_signal = cancel_tx_id
            .as_deref()
            .and_then(|tx_id| signals.get(tx_id).cloned());
        Self {
            cancel_tx_id,
            cancel_tx_signal,
            submitted_at: Some(row.updated_at.clone()),
        }
    }
}

/// Whether a missing watched offer in `cancel_submitted` should stay preserved.
#[must_use]
pub fn preserve_cancel_submitted_on_missing_offer() -> bool {
    true
}

/// Whether daemon cancel policy should skip targeting this offer for now.
#[must_use]
pub fn defer_cancel_target(ctx: &CancelSubmittedContext) -> bool {
    cancel_tx_in_flight(ctx)
}

/// Drop Dexie-open ids whose cancel submit is still in flight.
#[must_use]
pub fn filter_defer_cancel_submitted_targets(
    offer_ids: &[String],
    db_rows: &[OfferStateListRow],
    tx_signals: &HashMap<String, TxSignalStateRow>,
) -> Vec<String> {
    let defer_targets: std::collections::HashSet<&str> = db_rows
        .iter()
        .filter_map(|row| {
            if !ReconcileState::parse(&row.state).is_ok_and(|state| state.is_cancel_submitted()) {
                return None;
            }
            let ctx = CancelSubmittedContext::from_row_and_signals(row, tx_signals);
            defer_cancel_target(&ctx).then_some(row.offer_id.as_str())
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
    coinset: CoinsetOfferSignals,
    ctx: &CancelSubmittedContext,
) -> ReconcileTransition {
    if coinset.has_confirmed && dexie_status != Some(DEXIE_STATUS_CANCELLED) {
        return open_signal_transition(
            OfferSignal::TxConfirmed,
            REASON_COINSET_CONFIRMED,
            SIGNAL_SOURCE_COINSET_WEBHOOK,
            TAKER_COINSET_TX_BLOCK_WEBHOOK,
            TAKER_DIAGNOSTIC_COINSET_CONFIRMED,
        );
    }
    if coinset.has_mempool {
        return open_signal_transition(
            OfferSignal::MempoolSeen,
            REASON_COINSET_MEMPOOL,
            SIGNAL_SOURCE_COINSET_MEMPOOL,
            TAKER_NONE,
            TAKER_DIAGNOSTIC_COINSET_MEMPOOL,
        );
    }
    match dexie_status {
        None if coinset.has_tx_ids => {
            preserve_state(&ReconcileState::CancelSubmitted, REASON_COINSET_UNAVAILABLE)
        }
        None => preserve_state(&ReconcileState::CancelSubmitted, REASON_MISSING_STATUS),
        Some(DEXIE_STATUS_OPEN) if stale_cancel_submit_eligible(ctx) => ReconcileTransition::new(
            ReconcileState::Lifecycle(OfferLifecycleState::Open),
            REASON_CANCEL_SUBMIT_STALE_DEXIE_OPEN,
            SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK,
            None,
            TAKER_NONE,
            TAKER_NONE,
        ),
        Some(status) => dexie_fallback_for_cancel_submitted(status),
    }
}

fn dexie_fallback_for_cancel_submitted(status: i64) -> ReconcileTransition {
    let taker_diagnostic = if is_dexie_pattern_fallback_status(status) {
        TAKER_DIAGNOSTIC_DEXIE_PATTERN_FALLBACK
    } else {
        TAKER_NONE
    };
    match reconcile_from_dexie_status(status) {
        DexieStatusReconcile::Cancelled => {
            dexie_fallback_transition(ReconcileState::Cancelled, None, taker_diagnostic)
        }
        DexieStatusReconcile::ApplySignal(signal) => dexie_fallback_transition(
            ReconcileState::from_open_signal(signal),
            Some(signal),
            taker_diagnostic,
        ),
        DexieStatusReconcile::Unchanged => {
            dexie_fallback_transition(ReconcileState::CancelSubmitted, None, taker_diagnostic)
        }
    }
}

#[must_use]
fn cancel_tx_in_flight(ctx: &CancelSubmittedContext) -> bool {
    let Some(cancel_tx_id) = ctx
        .cancel_tx_id
        .as_deref()
        .filter(|value| !value.is_empty())
    else {
        return within_cancel_submit_grace(ctx.submitted_at.as_deref());
    };
    let _ = cancel_tx_id;
    match ctx.cancel_tx_signal.as_ref() {
        None => within_cancel_submit_grace(ctx.submitted_at.as_deref()),
        Some(signal) if signal.tx_block_confirmed_at.is_some() => false,
        Some(signal) if signal.mempool_observed_at.is_some() => true,
        Some(_) => false,
    }
}

#[must_use]
fn stale_cancel_submit_eligible(ctx: &CancelSubmittedContext) -> bool {
    !cancel_tx_in_flight(ctx)
}

#[must_use]
fn within_cancel_submit_grace(submitted_at: Option<&str>) -> bool {
    let Some(raw) = submitted_at
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    let Ok(parsed) = DateTime::parse_from_rfc3339(raw) else {
        return false;
    };
    (Utc::now() - parsed.with_timezone(&Utc)).num_seconds() < CANCEL_SUBMIT_TRACKING_GRACE_SECS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::OfferStateListRow;

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
        assert!(defer_cancel_target(&ctx));
    }

    #[test]
    fn stale_reset_when_cancel_tx_confirmed_but_dexie_still_open() {
        let ctx = CancelSubmittedContext {
            cancel_tx_id: Some("tx1".to_string()),
            cancel_tx_signal: Some(TxSignalStateRow {
                mempool_observed_at: Some("2020-01-01T00:00:00Z".to_string()),
                tx_block_confirmed_at: Some("2020-01-01T00:01:00Z".to_string()),
            }),
            submitted_at: None,
        };
        assert!(stale_cancel_submit_eligible(&ctx));
    }

    #[test]
    fn filter_defers_only_in_flight_cancel_submitted() {
        let now = Utc::now().to_rfc3339();
        let rows = vec![
            row("o1", "open", None, &now),
            row("o2", "cancel_submitted", Some("tx2"), &now),
        ];
        let mut signals = HashMap::new();
        signals.insert(
            "tx2".to_string(),
            TxSignalStateRow {
                mempool_observed_at: Some(now.clone()),
                tx_block_confirmed_at: None,
            },
        );
        assert_eq!(
            filter_defer_cancel_submitted_targets(
                &["o1".to_string(), "o2".to_string()],
                &rows,
                &signals,
            ),
            vec!["o1".to_string()]
        );
    }
}
