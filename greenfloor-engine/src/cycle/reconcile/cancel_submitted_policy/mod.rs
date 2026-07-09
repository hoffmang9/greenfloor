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
    REASON_CANCEL_SUBMIT_STALE_ORPHAN, REASON_CANCEL_SUBMIT_WATCH_HIT_IGNORED,
    REASON_COINSET_UNAVAILABLE, REASON_MISSING_STATUS, SIGNAL_SOURCE_NONE, TAKER_NONE,
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

#[must_use]
pub(crate) fn chain_confirmed_tx_ids_from_signals(
    signals: &HashMap<String, TxSignalStateRow>,
) -> Vec<String> {
    signals
        .iter()
        .filter(|(_, row)| row.tx_block_confirmed_at.is_some())
        .map(|(tx_id, _)| tx_id.clone())
        .collect()
}

/// Drop offer ids whose cancel submit is still in flight (pure policy; no I/O).
#[must_use]
pub(crate) fn allowed_cancel_target_offer_ids(
    offer_ids: &[String],
    db_rows: &[OfferStateListRow],
    tx_signals: &HashMap<String, TxSignalStateRow>,
    now: DateTime<Utc>,
) -> Vec<String> {
    let chain_confirmed = chain_confirmed_tx_ids_from_signals(tx_signals);
    let defer_targets: std::collections::HashSet<&str> = db_rows
        .iter()
        .filter_map(|row| {
            if !ReconcileState::parse(&row.state).is_ok_and(|state| state.is_cancel_submitted()) {
                return None;
            }
            if row
                .cancel_submitted_tx_id
                .as_deref()
                .map(str::trim)
                .is_none_or(str::is_empty)
            {
                return None;
            }
            let ctx = CancelSubmittedContext::from_row_and_signals(row, tx_signals);
            is_cancel_submit_in_flight(&ctx, now, &chain_confirmed).then_some(row.offer_id.as_str())
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
    summary: CoinsetSignalSummary,
    chain_confirmed_tx_ids: &[String],
    ctx: &CancelSubmittedContext,
    now: DateTime<Utc>,
) -> ReconcileTransition {
    let current = ReconcileState::CancelSubmitted;
    if cancel_tx_chain_confirmed(ctx, chain_confirmed_tx_ids) {
        return cancel_tx_chain_confirmed_transition();
    }
    if dexie_status == Some(DEXIE_STATUS_CANCELLED) {
        return transition_from_dexie_status(DEXIE_STATUS_CANCELLED, current);
    }
    // Watches stay registered through prepare/finalize; a pure watch hit must not
    // look like taker mempool activity while cancel_submitted is in flight.
    if summary.is_pure_watch_hit() {
        return preserve_state(&current, REASON_CANCEL_SUBMIT_WATCH_HIT_IGNORED);
    }
    if let Some(taker) = apply_coinset_taker_dispatch_if_present(summary, dexie_status, &current) {
        return taker;
    }
    cancel_submitted_status_fallback_transition(
        dexie_status,
        summary,
        ctx,
        now,
        chain_confirmed_tx_ids,
    )
}

fn cancel_submitted_status_fallback_transition(
    dexie_status: Option<i64>,
    summary: CoinsetSignalSummary,
    ctx: &CancelSubmittedContext,
    now: DateTime<Utc>,
    chain_confirmed_tx_ids: &[String],
) -> ReconcileTransition {
    if cancel_submit_stale_reset_eligible(ctx, now, chain_confirmed_tx_ids)
        && matches!(dexie_status, None | Some(DEXIE_STATUS_OPEN))
    {
        // Dexie open or Coinset/splash (no status): cancel never confirmed → retry.
        return ReconcileTransition::new(
            ReconcileState::Lifecycle(OfferLifecycleState::Open),
            REASON_CANCEL_SUBMIT_STALE_ORPHAN,
            SIGNAL_SOURCE_NONE,
            None,
            TAKER_NONE,
            TAKER_NONE,
        );
    }
    match dexie_status {
        None if summary.has_coinset_activity() => {
            preserve_state(&ReconcileState::CancelSubmitted, REASON_COINSET_UNAVAILABLE)
        }
        None => preserve_state(&ReconcileState::CancelSubmitted, REASON_MISSING_STATUS),
        Some(status) => transition_from_dexie_status(status, ReconcileState::CancelSubmitted),
    }
}

#[must_use]
pub(crate) fn cancel_tx_chain_confirmed(
    ctx: &CancelSubmittedContext,
    chain_confirmed_tx_ids: &[String],
) -> bool {
    if ctx
        .cancel_tx_signal
        .as_ref()
        .is_some_and(|signal| signal.tx_block_confirmed_at.is_some())
    {
        return true;
    }
    let Some(cancel_tx_id) = ctx.cancel_tx_id.as_deref().and_then(canonical_tx_id) else {
        return false;
    };
    chain_confirmed_tx_ids
        .iter()
        .any(|tx_id| canonical_tx_id(tx_id).as_deref() == Some(cancel_tx_id.as_str()))
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
fn is_cancel_submit_in_flight(
    ctx: &CancelSubmittedContext,
    now: DateTime<Utc>,
    chain_confirmed_tx_ids: &[String],
) -> bool {
    !cancel_tx_chain_confirmed(ctx, chain_confirmed_tx_ids) && cancel_submit_within_grace(ctx, now)
}

/// Unconfirmed cancel submit past orphan grace — eligible for stale reset to `open`.
#[must_use]
fn cancel_submit_stale_reset_eligible(
    ctx: &CancelSubmittedContext,
    now: DateTime<Utc>,
    chain_confirmed_tx_ids: &[String],
) -> bool {
    !cancel_tx_chain_confirmed(ctx, chain_confirmed_tx_ids) && !cancel_submit_within_grace(ctx, now)
}

#[cfg(test)]
mod tests;
