//! SQLite-backed assembly of [`CancelSubmittedContext`] for lifecycle reconcile.

use std::collections::HashMap;

use crate::cycle::reconcile::{CancelSubmittedContext, ReconcileState};
use crate::error::SignerResult;
use crate::hex::canonical_tx_id;
use crate::storage::{OfferStateListRow, SqliteStore};

/// Preload cancel-submit context for all `cancel_submitted` rows in one tx-signal query.
///
/// # Errors
///
/// Returns an error if tx signal lookup fails.
pub fn preload_cancel_submitted_contexts(
    store: &SqliteStore,
    rows: &[OfferStateListRow],
) -> SignerResult<HashMap<String, CancelSubmittedContext>> {
    let cancel_rows: Vec<&OfferStateListRow> = rows
        .iter()
        .filter(|row| {
            ReconcileState::parse(&row.state).is_ok_and(|state| state.is_cancel_submitted())
        })
        .collect();
    if cancel_rows.is_empty() {
        return Ok(HashMap::default());
    }
    let cancel_tx_ids: Vec<String> = cancel_rows
        .iter()
        .filter_map(|row| row.cancel_submitted_tx_id.clone())
        .collect();
    let tx_signals = store.get_tx_signal_state(&cancel_tx_ids)?;
    Ok(cancel_rows
        .into_iter()
        .map(|row| {
            (
                row.offer_id.clone(),
                CancelSubmittedContext::from_row_and_signals(row, &tx_signals),
            )
        })
        .collect())
}

/// Resolve cancel-submit context for one offer during reconcile.
///
/// # Errors
///
/// Returns an error if offer state or tx signal lookup fails.
pub fn cancel_submitted_context_for_offer(
    store: &SqliteStore,
    offer_id: &str,
    current_state: &str,
    preloaded: Option<&HashMap<String, CancelSubmittedContext>>,
) -> SignerResult<Option<CancelSubmittedContext>> {
    if !ReconcileState::parse(current_state).is_ok_and(|state| state.is_cancel_submitted()) {
        return Ok(None);
    }
    if let Some(map) = preloaded {
        return Ok(map.get(offer_id).cloned());
    }
    let rows = store.list_offer_states_for_ids(&[offer_id.to_string()])?;
    let Some(row) = rows.into_iter().next() else {
        return Ok(None);
    };
    if !ReconcileState::parse(&row.state).is_ok_and(|state| state.is_cancel_submitted()) {
        return Ok(None);
    }
    let tx_signals = match row.cancel_submitted_tx_id.as_deref().and_then(canonical_tx_id) {
        Some(canonical) => store.get_tx_signal_state(std::slice::from_ref(&canonical))?,
        None => HashMap::default(),
    };
    Ok(Some(CancelSubmittedContext::from_row_and_signals(
        &row, &tx_signals,
    )))
}
