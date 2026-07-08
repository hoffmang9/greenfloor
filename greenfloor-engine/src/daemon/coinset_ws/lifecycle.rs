//! Apply Coinset WS offer / watch signals through canonical reconcile decision.

use chrono::Utc;

use crate::cycle::reconcile::{
    resolve_coinset_mempool_hit_transition, resolve_watched_offer_transition_from_signals,
    CancelSubmittedContext, DexieCoinsetSignals,
};
use crate::daemon::coinset_tx::WsOfferEvent;
use crate::error::SignerResult;
use crate::offer::dexie_payload::{
    DEXIE_STATUS_CANCELLED, DEXIE_STATUS_CONFIRMED, DEXIE_STATUS_EXPIRED,
};
use crate::offer::lifecycle::{
    cancel_submitted_context_for_offer, persist_offer_lifecycle_transition, ReconcilePersistOptions,
};
use crate::storage::SqliteStore;

fn persist_changed(
    store: &SqliteStore,
    market_id: &str,
    offer_id: &str,
    transition: &crate::cycle::CycleOfferTransition,
) -> SignerResult<()> {
    if !transition.changed {
        return Ok(());
    }
    persist_offer_lifecycle_transition(
        store,
        market_id,
        offer_id,
        transition,
        None,
        &ReconcilePersistOptions {
            action: "coinset_ws_lifecycle",
            venue: Some("coinset"),
            dexie_error: None,
        },
    )
}

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

fn signals_for_offer_status(event: &WsOfferEvent) -> Option<(Option<i64>, DexieCoinsetSignals)> {
    let tx = event.tx_id.clone().into_iter().collect::<Vec<_>>();
    match event.status.as_str() {
        "pending" => Some((
            None,
            DexieCoinsetSignals {
                tx_ids: tx.clone(),
                confirmed_tx_ids: Vec::new(),
                mempool_tx_ids: tx,
            },
        )),
        "confirmed" => Some((
            Some(DEXIE_STATUS_CONFIRMED),
            DexieCoinsetSignals {
                tx_ids: tx.clone(),
                confirmed_tx_ids: tx,
                mempool_tx_ids: Vec::new(),
            },
        )),
        "expired" => Some((Some(DEXIE_STATUS_EXPIRED), DexieCoinsetSignals::default())),
        "cancelled" => Some((Some(DEXIE_STATUS_CANCELLED), DexieCoinsetSignals::default())),
        _ => None,
    }
}

fn apply_reconcile_signals(
    store: &SqliteStore,
    offer_id: &str,
    market_id: &str,
    current_state: &str,
    status: Option<i64>,
    dexie: DexieCoinsetSignals,
    cancel_submitted: Option<&CancelSubmittedContext>,
) -> SignerResult<()> {
    let transition = resolve_watched_offer_transition_from_signals(
        current_state,
        status,
        dexie,
        &[],
        cancel_submitted,
        Utc::now(),
    )
    .map_err(|err| crate::error::SignerError::Other(err.to_string()))?;
    persist_changed(store, market_id, offer_id, &transition)
}

/// Drive lifecycle from a Coinset WS `offer` event for a locally tracked offer.
pub fn apply_ws_offer_event(store: &SqliteStore, event: &WsOfferEvent) -> SignerResult<()> {
    seed_offer_tx_signal(store, event)?;
    let Some((status, dexie)) = signals_for_offer_status(event) else {
        return Ok(());
    };
    let rows = store.list_offer_states_for_ids(std::slice::from_ref(&event.offer_id))?;
    let Some(row) = rows.first() else {
        return Ok(());
    };
    let cancel_submitted =
        cancel_submitted_context_for_offer(store, &row.offer_id, &row.state, None)?;
    apply_reconcile_signals(
        store,
        &row.offer_id,
        &row.market_id,
        &row.state,
        status,
        dexie,
        cancel_submitted.as_ref(),
    )
}

/// On a durable coin/p2 watch hit, mark `mempool_observed` via reconcile dispatch.
pub fn apply_watch_hit_mempool(store: &SqliteStore, watched_key: &str) -> SignerResult<()> {
    let offer_ids = store.list_offer_ids_for_watched_coin(watched_key)?;
    if offer_ids.is_empty() {
        return Ok(());
    }
    for row in store.list_offer_states_for_ids(&offer_ids)? {
        let cancel_submitted =
            cancel_submitted_context_for_offer(store, &row.offer_id, &row.state, None)?;
        let transition = resolve_coinset_mempool_hit_transition(
            &row.state,
            cancel_submitted.as_ref(),
            Utc::now(),
        )
        .map_err(|err| crate::error::SignerError::Other(err.to_string()))?;
        persist_changed(store, &row.market_id, &row.offer_id, &transition)?;
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
        apply_watch_hit_mempool(&store, &coin).expect("hit");
        let rows = store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows");
        assert_eq!(rows[0].state, "mempool_observed");
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
