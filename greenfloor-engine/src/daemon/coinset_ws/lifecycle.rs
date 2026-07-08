//! Apply Coinset WS offer / watch signals via reconcile builders.

use crate::cycle::lifecycle::OfferSignal;
use crate::cycle::reconcile::{
    open_signal_transition, CycleOfferTransition, ReconcileState, ReconcileTransition,
    REASON_COINSET_CONFIRMED, REASON_COINSET_MEMPOOL, REASON_OK, SIGNAL_SOURCE_COINSET_WEBSOCKET,
    TAKER_COINSET_TX_BLOCK_WEBSOCKET, TAKER_DIAGNOSTIC_COINSET_CONFIRMED,
    TAKER_DIAGNOSTIC_COINSET_MEMPOOL, TAKER_NONE,
};
use crate::daemon::coinset_tx::WsOfferEvent;
use crate::error::SignerResult;
use crate::offer::lifecycle::{persist_offer_lifecycle_transition, ReconcilePersistOptions};
use crate::storage::SqliteStore;

struct WsSignal {
    signal: OfferSignal,
    reason: &'static str,
    taker_signal: &'static str,
    taker_diagnostic: &'static str,
    tx_ids: Vec<String>,
}

fn persist_changed(
    store: &SqliteStore,
    market_id: &str,
    offer_id: &str,
    transition: &CycleOfferTransition,
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

fn ws_open_signal(old_state: ReconcileState, apply: WsSignal) -> CycleOfferTransition {
    let (confirmed, mempool) = match apply.signal {
        OfferSignal::TxConfirmed => (apply.tx_ids.clone(), Vec::new()),
        OfferSignal::MempoolSeen => (Vec::new(), apply.tx_ids.clone()),
        _ => (Vec::new(), Vec::new()),
    };
    open_signal_transition(
        apply.signal,
        apply.reason,
        SIGNAL_SOURCE_COINSET_WEBSOCKET,
        apply.taker_signal,
        apply.taker_diagnostic,
    )
    .into_cycle_transition(old_state, apply.tx_ids, confirmed, mempool)
}

fn apply_signal(
    store: &SqliteStore,
    offer_id: &str,
    market_id: &str,
    current_state: &str,
    apply: WsSignal,
) -> SignerResult<()> {
    let Ok(old_state) = ReconcileState::parse(current_state) else {
        return Ok(());
    };
    if old_state.is_terminal() || old_state.is_cancel_submitted() {
        return Ok(());
    }
    persist_changed(
        store,
        market_id,
        offer_id,
        &ws_open_signal(old_state, apply),
    )
}

/// Drive lifecycle from a Coinset WS `offer` event for a locally tracked offer.
pub fn apply_ws_offer_event(store: &SqliteStore, event: &WsOfferEvent) -> SignerResult<()> {
    let rows = store.list_offer_states_for_ids(std::slice::from_ref(&event.offer_id))?;
    let Some(row) = rows.first() else {
        return Ok(());
    };
    let Ok(old_state) = ReconcileState::parse(&row.state) else {
        return Ok(());
    };
    if old_state.is_terminal() {
        return Ok(());
    }

    let tx_ids = event.tx_id.clone().into_iter().collect::<Vec<_>>();
    let transition = match event.status.as_str() {
        "pending" | "confirmed" | "expired" if old_state.is_cancel_submitted() => return Ok(()),
        "pending" => ws_open_signal(
            old_state,
            WsSignal {
                signal: OfferSignal::MempoolSeen,
                reason: REASON_COINSET_MEMPOOL,
                taker_signal: TAKER_NONE,
                taker_diagnostic: TAKER_DIAGNOSTIC_COINSET_MEMPOOL,
                tx_ids,
            },
        ),
        "confirmed" => ws_open_signal(
            old_state,
            WsSignal {
                signal: OfferSignal::TxConfirmed,
                reason: REASON_COINSET_CONFIRMED,
                taker_signal: TAKER_COINSET_TX_BLOCK_WEBSOCKET,
                taker_diagnostic: TAKER_DIAGNOSTIC_COINSET_CONFIRMED,
                tx_ids,
            },
        ),
        "expired" => ws_open_signal(
            old_state,
            WsSignal {
                signal: OfferSignal::Expired,
                reason: REASON_OK,
                taker_signal: TAKER_NONE,
                taker_diagnostic: TAKER_NONE,
                tx_ids: Vec::new(),
            },
        ),
        "cancelled" => ReconcileTransition::new(
            ReconcileState::Cancelled,
            REASON_OK,
            SIGNAL_SOURCE_COINSET_WEBSOCKET,
            None,
            TAKER_NONE,
            TAKER_NONE,
        )
        .into_cycle_transition_no_coinset(old_state),
        _ => return Ok(()),
    };
    persist_changed(store, &row.market_id, &row.offer_id, &transition)
}

/// On a durable coin/`p2` watch hit, mark `mempool_observed` for the watching offer(s).
pub fn apply_watch_hit_mempool(store: &SqliteStore, watched_key: &str) -> SignerResult<()> {
    let offer_ids = store.list_offer_ids_for_watched_coin(watched_key)?;
    if offer_ids.is_empty() {
        return Ok(());
    }
    for row in store.list_offer_states_for_ids(&offer_ids)? {
        apply_signal(
            store,
            &row.offer_id,
            &row.market_id,
            &row.state,
            WsSignal {
                signal: OfferSignal::MempoolSeen,
                reason: REASON_COINSET_MEMPOOL,
                taker_signal: TAKER_NONE,
                taker_diagnostic: TAKER_DIAGNOSTIC_COINSET_MEMPOOL,
                tx_ids: Vec::new(),
            },
        )?;
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
        store
            .upsert_offer_state(&offer_id, "m1", "open", None)
            .expect("upsert");
        apply_ws_offer_event(
            &store,
            &WsOfferEvent {
                offer_id: offer_id.clone(),
                status: "pending".to_string(),
                tx_id: Some("cd".repeat(32)),
                p2s: Vec::new(),
            },
        )
        .expect("apply");
        let rows = store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows");
        assert_eq!(rows[0].state, "mempool_observed");
    }

    #[test]
    fn offer_confirmed_moves_to_tx_block_confirmed() {
        let (_dir, store) = open_store();
        let offer_id = "ab".repeat(32);
        store
            .upsert_offer_state(&offer_id, "m1", "mempool_observed", None)
            .expect("upsert");
        apply_ws_offer_event(
            &store,
            &WsOfferEvent {
                offer_id: offer_id.clone(),
                status: "confirmed".to_string(),
                tx_id: Some("cd".repeat(32)),
                p2s: Vec::new(),
            },
        )
        .expect("apply");
        let rows = store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows");
        assert_eq!(rows[0].state, "tx_block_confirmed");
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
}
