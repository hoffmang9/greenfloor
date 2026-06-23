//! SQLite-backed assembly of [`CancelSubmittedContext`] for lifecycle reconcile.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use crate::cycle::reconcile::{
    allowed_cancel_target_offer_ids, cancel_tx_chain_confirmed, CancelSubmittedContext,
    ReconcileState,
};
use crate::error::SignerResult;
use crate::hex::canonical_tx_id;
use crate::storage::{OfferStateListRow, SqliteStore, TxSignalStateRow};

/// CLI/daemon skip reason when a tracked offer's cancel submit is still in flight.
pub const CANCEL_SUBMIT_IN_FLIGHT_SKIP_REASON: &str = "cancel_submit_in_flight";

/// Merged chain-confirmed tx ids for reconcile (Dexie-linked + tracked cancel tx).
///
/// Refreshes the tracked cancel tx signal from `SQLite` when preloaded context is stale.
///
/// # Errors
///
/// Returns an error if cancel tx signal lookup fails.
pub fn chain_confirmed_tx_ids_for_transition(
    store: &SqliteStore,
    cancel_submitted: Option<&CancelSubmittedContext>,
    dexie_confirmed_tx_ids: &[String],
) -> SignerResult<Vec<String>> {
    let mut chain_confirmed = dexie_confirmed_tx_ids.to_vec();
    let Some(ctx) = cancel_submitted else {
        return Ok(chain_confirmed);
    };
    if cancel_tx_chain_confirmed(ctx, &chain_confirmed) {
        return Ok(chain_confirmed);
    }
    let Some(cancel_id) = ctx.cancel_tx_id.as_deref().and_then(canonical_tx_id) else {
        return Ok(chain_confirmed);
    };
    let signals = store.get_tx_signal_state(std::slice::from_ref(&cancel_id))?;
    if signals
        .get(&cancel_id)
        .is_some_and(|row| row.tx_block_confirmed_at.is_some())
    {
        chain_confirmed.push(cancel_id);
    }
    Ok(chain_confirmed)
}

#[derive(Debug, Clone)]
pub struct DeferInFlightCancelPartition<T> {
    pub active: Vec<T>,
    pub skipped: Vec<T>,
}

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
    Ok(allowed_cancel_target_offer_ids(
        offer_ids,
        db_rows,
        &tx_signals,
        now,
    ))
}

/// Split cancel targets into active vs in-flight-deferred buckets.
///
/// # Errors
///
/// Returns an error if tx signal lookup fails.
pub fn partition_defer_in_flight_cancel_targets<T>(
    store: &SqliteStore,
    rows: &[OfferStateListRow],
    targets: Vec<T>,
    now: DateTime<Utc>,
    offer_id: impl Fn(&T) -> &str,
    persists_state: impl Fn(&T) -> bool,
) -> SignerResult<DeferInFlightCancelPartition<T>> {
    if targets.is_empty() {
        return Ok(DeferInFlightCancelPartition {
            active: Vec::new(),
            skipped: Vec::new(),
        });
    }
    let tracked_ids: Vec<String> = targets
        .iter()
        .filter(|target| persists_state(target))
        .map(|target| offer_id(target).to_string())
        .collect();
    let allowed = defer_in_flight_cancel_offer_ids(store, rows, &tracked_ids, now)?;
    let allowed: HashSet<&str> = allowed.iter().map(String::as_str).collect();
    let mut partition = DeferInFlightCancelPartition {
        active: Vec::new(),
        skipped: Vec::new(),
    };
    for target in targets {
        if persists_state(&target) && !allowed.contains(offer_id(&target)) {
            partition.skipped.push(target);
        } else {
            partition.active.push(target);
        }
    }
    Ok(partition)
}

/// JSON result payload for CLI items skipped due to in-flight cancel submit.
#[must_use]
pub fn cancel_submit_in_flight_skip_result() -> Value {
    json!({
        "skipped": true,
        "reason": CANCEL_SUBMIT_IN_FLIGHT_SKIP_REASON,
    })
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
        if let Some(ctx) = map.get(offer_id) {
            return Ok(Some(ctx.clone()));
        }
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
    use crate::storage::TxSignalStateRow;

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

    #[test]
    fn partition_defers_tracked_in_flight_cancel_submitted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let tx_id = "b".repeat(64);
        store
            .upsert_offer_cancel_submitted("offer-defer", "m1", &tx_id, Some(0))
            .expect("seed");
        let rows = store
            .list_offer_states_for_ids(&["offer-defer".to_string()])
            .expect("rows");
        let now = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
        let partition = partition_defer_in_flight_cancel_targets(
            &store,
            &rows,
            vec![("offer-defer".to_string(), true)],
            now,
            |target| target.0.as_str(),
            |target| target.1,
        )
        .expect("partition");
        assert!(partition.active.is_empty());
        assert_eq!(partition.skipped.len(), 1);
        assert_eq!(
            cancel_submit_in_flight_skip_result().get("reason"),
            Some(&json!(CANCEL_SUBMIT_IN_FLIGHT_SKIP_REASON))
        );
    }

    #[test]
    fn cancel_submitted_context_falls_back_when_preload_misses_offer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let tx_id = "d".repeat(64);
        store
            .upsert_offer_cancel_submitted("offer-preload-miss", "m1", &tx_id, Some(0))
            .expect("seed");
        let preloaded = HashMap::new();
        let ctx = cancel_submitted_context_for_offer(
            &store,
            "offer-preload-miss",
            "cancel_submitted",
            Some(&preloaded),
        )
        .expect("context")
        .expect("cancel context");
        assert_eq!(ctx.cancel_tx_id.as_deref(), Some(tx_id.as_str()));
    }

    #[test]
    fn chain_confirmed_for_transition_refreshes_stale_cancel_tx_signal() {
        let tx_id = "d".repeat(64);
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        store
            .observe_mempool_tx_ids(std::slice::from_ref(&tx_id))
            .expect("mempool");
        store
            .confirm_tx_ids(std::slice::from_ref(&tx_id))
            .expect("confirm");
        let ctx = CancelSubmittedContext {
            cancel_tx_id: Some(tx_id.clone()),
            cancel_tx_signal: Some(TxSignalStateRow {
                mempool_observed_at: Some("2020-01-01T00:00:00Z".to_string()),
                tx_block_confirmed_at: None,
            }),
            cancel_submitted_at: None,
        };
        let chain =
            chain_confirmed_tx_ids_for_transition(&store, Some(&ctx), &[]).expect("confirmed");
        assert!(chain.iter().any(|id| id == &tx_id));
    }
}
