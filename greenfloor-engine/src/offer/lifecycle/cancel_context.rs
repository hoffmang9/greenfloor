//! SQLite-backed assembly of [`CancelSubmittedContext`] for lifecycle reconcile.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::cycle::reconcile::{
    filter_defer_cancel_submitted_targets, CancelSubmittedContext, ReconcileState,
};
use crate::error::SignerResult;
use crate::hex::canonical_tx_id;
use crate::storage::{OfferStateListRow, SqliteStore, TxSignalStateRow};

fn cancel_tx_signals_for_rows(
    store: &SqliteStore,
    rows: &[OfferStateListRow],
) -> SignerResult<HashMap<String, TxSignalStateRow>> {
    let cancel_tx_ids: Vec<String> = rows
        .iter()
        .filter_map(|row| row.cancel_submitted_tx_id.clone())
        .collect();
    store.get_tx_signal_state(&cancel_tx_ids)
}

/// Drop offer ids whose cancel submit is still in flight (daemon + CLI cancel targeting).
///
/// # Errors
///
/// Returns an error if tx signal lookup fails.
pub fn defer_in_flight_cancel_offer_ids(
    store: &SqliteStore,
    db_rows: &[OfferStateListRow],
    offer_ids: &[String],
    now: DateTime<Utc>,
) -> SignerResult<Vec<String>> {
    if offer_ids.is_empty() {
        return Ok(Vec::new());
    }
    let tx_signals = cancel_tx_signals_for_rows(store, db_rows)?;
    Ok(filter_defer_cancel_submitted_targets(
        offer_ids,
        db_rows,
        &tx_signals,
        now,
    ))
}

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
    let tx_signals = cancel_tx_signals_for_rows(store, rows)?;
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
    let tx_signals = match row
        .cancel_submitted_tx_id
        .as_deref()
        .and_then(canonical_tx_id)
    {
        Some(canonical) => store.get_tx_signal_state(std::slice::from_ref(&canonical))?,
        None => HashMap::default(),
    };
    Ok(Some(CancelSubmittedContext::from_row_and_signals(
        &row,
        &tx_signals,
    )))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn defer_in_flight_cancel_offer_ids_skips_pending_cancel_submitted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let tx_id = "c".repeat(64);
        store
            .upsert_offer_cancel_submitted("offer-defer", "m1", &tx_id, Some(0))
            .expect("seed");
        let rows = store
            .list_offer_states_for_ids(&["offer-defer".to_string()])
            .expect("rows");
        let now = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
        let allowed =
            defer_in_flight_cancel_offer_ids(&store, &rows, &["offer-defer".to_string()], now)
                .expect("defer");
        assert!(allowed.is_empty());
    }
}
