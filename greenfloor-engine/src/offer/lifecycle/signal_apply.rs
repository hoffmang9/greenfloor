//! Shared resolve+persist for watched-offer Coinset signals.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::cycle::reconcile::{
    resolve_watched_offer_transition_from_signals, CancelSubmittedContext, CoinsetTxSignals,
};
use crate::cycle::CycleOfferTransition;
use crate::error::SignerResult;
use crate::storage::{OfferStateListRow, SqliteStore};

use super::cancel_context::{
    cancel_submitted_context_for_offer, chain_confirmed_tx_ids_for_transition,
    preload_cancel_submitted_contexts,
};
use super::persist::{persist_offer_lifecycle_transition, ReconcilePersistOptions};

/// Persist an already-resolved watched-offer transition when it changes state or
/// carries a venue status touch.
///
/// # Errors
///
/// Returns an error if `SQLite` persist fails.
pub fn persist_resolved_watched_transition(
    store: &SqliteStore,
    market_id: &str,
    offer_id: &str,
    transition: &CycleOfferTransition,
    last_seen_status: Option<i64>,
    options: &ReconcilePersistOptions<'_>,
) -> SignerResult<()> {
    if transition.changed || last_seen_status.is_some() {
        persist_offer_lifecycle_transition(
            store,
            market_id,
            offer_id,
            transition,
            last_seen_status,
            options,
        )?;
    }
    Ok(())
}

/// Resolve watched-offer transition from signals and persist when needed.
///
/// Merges `confirmed_tx_ids` with the tracked cancel tx via
/// [`chain_confirmed_tx_ids_for_transition`] so WS and Dexie share one promotion path.
/// Persists when state changes **or** `last_seen_status` is present (Dexie status touch).
///
/// # Errors
///
/// Returns an error if reconcile or `SQLite` persist fails.
#[allow(clippy::too_many_arguments)]
pub fn apply_watched_offer_signals(
    store: &SqliteStore,
    market_id: &str,
    offer_id: &str,
    current_state: &str,
    status: Option<i64>,
    signals: CoinsetTxSignals,
    cancel_submitted: Option<&CancelSubmittedContext>,
    options: &ReconcilePersistOptions<'_>,
    last_seen_status: Option<i64>,
    now: DateTime<Utc>,
) -> SignerResult<CycleOfferTransition> {
    let chain_confirmed =
        chain_confirmed_tx_ids_for_transition(store, cancel_submitted, &signals.confirmed_tx_ids)?;
    let transition = resolve_watched_offer_transition_from_signals(
        current_state,
        status,
        signals,
        &chain_confirmed,
        cancel_submitted,
        now,
    )
    .map_err(|err| crate::error::SignerError::Other(err.to_string()))?;
    persist_resolved_watched_transition(
        store,
        market_id,
        offer_id,
        &transition,
        last_seen_status,
        options,
    )?;
    Ok(transition)
}

/// Apply signals to one offer-state row (shared WS / cancel-submitted seam).
///
/// # Errors
///
/// Returns an error if `SQLite` or reconcile persist fails.
pub fn apply_signals_to_row(
    store: &SqliteStore,
    row: &OfferStateListRow,
    status: Option<i64>,
    signals: CoinsetTxSignals,
    cancel_by_offer: Option<&HashMap<String, CancelSubmittedContext>>,
    options: &ReconcilePersistOptions<'_>,
    now: DateTime<Utc>,
) -> SignerResult<bool> {
    let cancel_submitted =
        cancel_submitted_context_for_offer(store, &row.offer_id, &row.state, cancel_by_offer)?;
    let transition = apply_watched_offer_signals(
        store,
        &row.market_id,
        &row.offer_id,
        &row.state,
        status,
        signals,
        cancel_submitted.as_ref(),
        options,
        None,
        now,
    )?;
    Ok(transition.changed)
}

/// Apply empty-signal cancel-submitted policy to rows (orphan unwedge / cancel-tx promote).
///
/// Past orphan grace, unconfirmed cancels reset to `open`. Within grace, non-attributable
/// noise still preserves `cancel_submitted`. Callers that already ingested confirmed cancel
/// txs rely on preloaded context seeing `tx_block_confirmed_at` for promotion.
///
/// # Errors
///
/// Returns an error if `SQLite` or reconcile persist fails.
pub fn apply_cancel_submitted_rows(
    store: &SqliteStore,
    rows: &[OfferStateListRow],
    options: &ReconcilePersistOptions<'_>,
    now: DateTime<Utc>,
) -> SignerResult<u64> {
    if rows.is_empty() {
        return Ok(0);
    }
    let cancel_by_offer = preload_cancel_submitted_contexts(store, rows)?;
    let mut changed = 0_u64;
    for row in rows {
        if apply_signals_to_row(
            store,
            row,
            None,
            CoinsetTxSignals::default(),
            Some(&cancel_by_offer),
            options,
            now,
        )? {
            changed += 1;
        }
    }
    Ok(changed)
}
