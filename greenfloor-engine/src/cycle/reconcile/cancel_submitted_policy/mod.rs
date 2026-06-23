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

/// Grace period before treating an unconfirmed cancel submit as stale.
pub(crate) const CANCEL_SUBMIT_TRACKING_GRACE_SECS: i64 = 5 * 60;

#[derive(Debug, Clone, Default)]
pub struct CancelSubmittedContext {
    pub cancel_tx_id: Option<String>,
    pub cancel_tx_signal: Option<TxSignalStateRow>,
    pub cancel_submitted_at: Option<String>,
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
            cancel_submitted_at: row.cancel_submitted_at.clone(),
        }
    }
}

/// Drop offer ids whose cancel submit is still in flight (pure policy; no I/O).
#[must_use]
pub(crate) fn allowed_cancel_target_offer_ids(
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
            is_cancel_submit_in_flight(&ctx, now).then_some(row.offer_id.as_str())
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
        Some(DEXIE_STATUS_OPEN) if cancel_submit_stale_reset_eligible(ctx, now) => {
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
fn cancel_submit_grace_anchor(ctx: &CancelSubmittedContext) -> Option<&str> {
    ctx.cancel_submitted_at.as_deref().or_else(|| {
        ctx.cancel_tx_signal
            .as_ref()
            .and_then(|signal| signal.mempool_observed_at.as_deref())
    })
}

#[must_use]
fn cancel_submit_within_grace(ctx: &CancelSubmittedContext, now: DateTime<Utc>) -> bool {
    cancel_submit_within_grace_at(cancel_submit_grace_anchor(ctx), now)
}

#[must_use]
fn cancel_submit_within_grace_at(cancel_submitted_at: Option<&str>, now: DateTime<Utc>) -> bool {
    let Some(raw) = cancel_submitted_at
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
fn is_cancel_submit_in_flight(ctx: &CancelSubmittedContext, now: DateTime<Utc>) -> bool {
    !cancel_tx_confirmed(ctx) && cancel_submit_within_grace(ctx, now)
}

/// Unconfirmed cancel submit past orphan grace — eligible for stale reset to `open`.
#[must_use]
fn cancel_submit_stale_reset_eligible(ctx: &CancelSubmittedContext, now: DateTime<Utc>) -> bool {
    !cancel_tx_confirmed(ctx) && !cancel_submit_within_grace(ctx, now)
}

#[cfg(test)]
mod tests;
