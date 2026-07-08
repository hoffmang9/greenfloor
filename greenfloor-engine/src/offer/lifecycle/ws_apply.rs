//! Apply Coinset WS offer / watch signals through canonical reconcile decision.

use crate::coinset::WsOfferEvent;
use crate::cycle::reconcile::{
    signals_from_ws_offer_status, CancelSubmittedContext, CoinsetSignalSummary, CoinsetTxSignals,
};
use crate::error::SignerResult;
use crate::storage::{OfferStateListRow, SqliteStore};

use super::cancel_context::{
    cancel_submitted_context_for_offer, preload_cancel_submitted_contexts,
};
use super::persist::ReconcilePersistOptions;
use super::signal_apply::apply_watched_offer_signals;

fn seed_offer_tx_signal(store: &SqliteStore, event: &WsOfferEvent) -> SignerResult<()> {
    let Some(tx_id) = event.tx_id.as_ref() else {
        return Ok(());
    };
    match event.status.as_str() {
        "confirmed" => {
            // confirm_tx_ids only updates existing rows; observe first so a
            // first-seen confirmed offer still seeds tx_signal_state.
            store.observe_mempool_tx_ids(std::slice::from_ref(tx_id))?;
            store.confirm_tx_ids(std::slice::from_ref(tx_id))?;
        }
        "pending" | "cancel_pending" => {
            store.observe_mempool_tx_ids(std::slice::from_ref(tx_id))?;
        }
        _ => {}
    }
    Ok(())
}

fn ws_persist_options() -> ReconcilePersistOptions<'static> {
    ReconcilePersistOptions {
        action: "coinset_ws_lifecycle",
        venue: Some("coinset"),
        dexie_error: None,
    }
}

/// Drive lifecycle from a Coinset WS `offer` event for a locally tracked offer.
///
/// # Errors
///
/// Returns an error if `SQLite` or reconcile persist fails.
pub fn apply_ws_offer_event(store: &SqliteStore, event: &WsOfferEvent) -> SignerResult<()> {
    seed_offer_tx_signal(store, event)?;
    let Some((status, signals)) =
        signals_from_ws_offer_status(&event.status, event.tx_id.as_deref())
    else {
        return Ok(());
    };
    let rows = store.list_offer_states_for_ids(std::slice::from_ref(&event.offer_id))?;
    let Some(row) = rows.first() else {
        return Ok(());
    };
    let cancel_submitted =
        cancel_submitted_context_for_offer(store, &row.offer_id, &row.state, None)?;
    apply_watched_offer_signals(
        store,
        &row.market_id,
        &row.offer_id,
        &row.state,
        status,
        signals,
        None,
        cancel_submitted.as_ref(),
        &ws_persist_options(),
    )
}

fn apply_mempool_hit_for_row(
    store: &SqliteStore,
    row: &OfferStateListRow,
    cancel_by_offer: &std::collections::HashMap<String, CancelSubmittedContext>,
) -> SignerResult<()> {
    let cancel_submitted = cancel_submitted_context_for_offer(
        store,
        &row.offer_id,
        &row.state,
        Some(cancel_by_offer),
    )?;
    apply_watched_offer_signals(
        store,
        &row.market_id,
        &row.offer_id,
        &row.state,
        None,
        CoinsetTxSignals::default(),
        Some(CoinsetSignalSummary::mempool_hit()),
        cancel_submitted.as_ref(),
        &ws_persist_options(),
    )
}

/// On durable coin/p2 watch hits, mark `mempool_observed` via reconcile dispatch (batched).
///
/// # Errors
///
/// Returns an error if `SQLite` or reconcile persist fails.
pub fn apply_watch_hits_batch(store: &SqliteStore, watched_keys: &[String]) -> SignerResult<()> {
    if watched_keys.is_empty() {
        return Ok(());
    }
    let rows = store.list_offer_states_for_watched_keys(watched_keys)?;
    if rows.is_empty() {
        return Ok(());
    }
    let cancel_by_offer = preload_cancel_submitted_contexts(store, &rows)?;
    for row in &rows {
        apply_mempool_hit_for_row(store, row, &cancel_by_offer)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn open_store() -> (tempfile::TempDir, SqliteStore) {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        (dir, store)
    }

    #[test]
    fn offer_pending_moves_open_to_mempool_observed() {
        let (_dir, store) = open_store();
        let offer_id = "ab".repeat(32);
        let tx_id = "cd".repeat(32);
        store
            .upsert_offer_state(&offer_id, "m1", "open", None)
            .expect("upsert");
        apply_ws_offer_event(
            &store,
            &WsOfferEvent {
                offer_id: offer_id.clone(),
                status: "pending".to_string(),
                tx_id: Some(tx_id.clone()),
                p2s: Vec::new(),
            },
        )
        .expect("apply");
        let rows = store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows");
        assert_eq!(rows[0].state, "mempool_observed");
        let signals = store
            .get_tx_signal_state(std::slice::from_ref(&tx_id))
            .expect("tx signals");
        let signal = signals.get(&tx_id).expect("seeded tx");
        assert!(signal.mempool_observed_at.is_some());
        assert!(signal.tx_block_confirmed_at.is_none());
    }

    #[test]
    fn offer_confirmed_moves_to_tx_block_confirmed() {
        let (_dir, store) = open_store();
        let offer_id = "ab".repeat(32);
        let tx_id = "cd".repeat(32);
        store
            .upsert_offer_state(&offer_id, "m1", "mempool_observed", None)
            .expect("upsert");
        apply_ws_offer_event(
            &store,
            &WsOfferEvent {
                offer_id: offer_id.clone(),
                status: "confirmed".to_string(),
                tx_id: Some(tx_id.clone()),
                p2s: Vec::new(),
            },
        )
        .expect("apply");
        let rows = store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows");
        assert_eq!(rows[0].state, "tx_block_confirmed");
        let signals = store
            .get_tx_signal_state(std::slice::from_ref(&tx_id))
            .expect("tx signals");
        let signal = signals.get(&tx_id).expect("seeded tx");
        assert!(signal.tx_block_confirmed_at.is_some());
    }

    #[test]
    fn offer_cancel_pending_seeds_tx_signal_without_state_change() {
        let (_dir, store) = open_store();
        let offer_id = "ab".repeat(32);
        let tx_id = "cd".repeat(32);
        store
            .upsert_offer_state(&offer_id, "m1", "open", None)
            .expect("upsert");
        apply_ws_offer_event(
            &store,
            &WsOfferEvent {
                offer_id: offer_id.clone(),
                status: "cancel_pending".to_string(),
                tx_id: Some(tx_id.clone()),
                p2s: Vec::new(),
            },
        )
        .expect("apply");
        let rows = store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows");
        assert_eq!(rows[0].state, "open");
        let signals = store
            .get_tx_signal_state(std::slice::from_ref(&tx_id))
            .expect("tx signals");
        let signal = signals.get(&tx_id).expect("seeded tx");
        assert!(signal.mempool_observed_at.is_some());
        assert!(signal.tx_block_confirmed_at.is_none());
    }

    #[test]
    fn watch_hit_marks_mempool_observed() {
        let (_dir, store) = open_store();
        let offer_id = "ab".repeat(32);
        let coin = "ef".repeat(32);
        store
            .upsert_offer_state(&offer_id, "m1", "open", None)
            .expect("upsert");
        store
            .replace_offer_coin_watches(&offer_id, "m1", std::slice::from_ref(&coin), &[])
            .expect("watch");
        apply_watch_hits_batch(&store, std::slice::from_ref(&coin)).expect("hit");
        let rows = store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows");
        assert_eq!(rows[0].state, "mempool_observed");
    }

    #[test]
    fn watch_hits_batch_updates_multiple_offers_and_dedupes_keys() {
        let (_dir, store) = open_store();
        let offer_a = "aa".repeat(32);
        let offer_b = "bb".repeat(32);
        let coin_a = "11".repeat(32);
        let coin_b = "22".repeat(32);
        let p2 = "33".repeat(32);
        for (offer_id, coins, p2s) in [
            (&offer_a, vec![coin_a.clone()], vec![p2.clone()]),
            (&offer_b, vec![coin_b.clone()], Vec::new()),
        ] {
            store
                .upsert_offer_state(offer_id, "m1", "open", None)
                .expect("upsert");
            store
                .replace_offer_coin_watches(offer_id, "m1", &coins, &p2s)
                .expect("watch");
        }
        // Same offer matched by coin + p2; second offer by coin only.
        apply_watch_hits_batch(&store, &[coin_a, p2, coin_b]).expect("batch");
        let rows = store
            .list_offer_states_for_ids(&[offer_a.clone(), offer_b.clone()])
            .expect("rows");
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.state == "mempool_observed"));
    }

    #[test]
    fn watch_hit_during_cancel_submitted_uses_preloaded_context() {
        let (_dir, store) = open_store();
        let offer_id = "ab".repeat(32);
        let coin = "ef".repeat(32);
        let cancel_tx = "cd".repeat(32);
        store
            .upsert_offer_cancel_submitted(&offer_id, "m1", &cancel_tx, None)
            .expect("cancel_submitted");
        store
            .replace_offer_coin_watches(&offer_id, "m1", std::slice::from_ref(&coin), &[])
            .expect("watch");
        apply_watch_hits_batch(&store, std::slice::from_ref(&coin)).expect("hit");
        let rows = store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows");
        // Synthetic mempool hit with cancel context: taker mempool path or preserve.
        assert!(
            matches!(
                rows[0].state.as_str(),
                "cancel_submitted" | "mempool_observed"
            ),
            "unexpected state {}",
            rows[0].state
        );
    }

    #[test]
    fn offer_confirmed_during_cancel_submitted_applies_taker() {
        let (_dir, store) = open_store();
        let offer_id = "ab".repeat(32);
        let cancel_tx = "cd".repeat(32);
        store
            .upsert_offer_cancel_submitted(&offer_id, "m1", &cancel_tx, None)
            .expect("cancel_submitted");
        apply_ws_offer_event(
            &store,
            &WsOfferEvent {
                offer_id: offer_id.clone(),
                status: "confirmed".to_string(),
                tx_id: Some("ef".repeat(32)),
                p2s: Vec::new(),
            },
        )
        .expect("apply");
        let rows = store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows");
        assert_eq!(rows[0].state, "tx_block_confirmed");
    }
}
