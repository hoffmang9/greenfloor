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
        .expect("signal");
    assert!(signals[&tx_id].mempool_observed_at.is_some());
    assert!(signals[&tx_id].tx_block_confirmed_at.is_none());
}

#[test]
fn offer_confirmed_moves_to_tx_block_confirmed() {
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
    let signal = store
        .get_tx_signal_state(std::slice::from_ref(&tx_id))
        .expect("signal")[&tx_id]
        .clone();
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
    let signal = store
        .get_tx_signal_state(std::slice::from_ref(&tx_id))
        .expect("signal")[&tx_id]
        .clone();
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
    apply_watch_hits_batch(&store, &[coin_a, p2, coin_b]).expect("batch");
    let rows = store
        .list_offer_states_for_ids(&[offer_a.clone(), offer_b.clone()])
        .expect("rows");
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row.state == "mempool_observed"));
}

#[test]
fn cancel_submitted_watch_hits_are_preserved_by_policy() {
    let (_dir, store) = open_store();
    let offer_id = "ab".repeat(32);
    let coin = "ef".repeat(32);
    let cancel_tx = "cd".repeat(32);
    store
        .upsert_offer_state(&offer_id, "m1", "open", None)
        .expect("upsert");
    store
        .replace_offer_coin_watches(&offer_id, "m1", std::slice::from_ref(&coin), &[])
        .expect("watch");
    store
        .prepare_offer_cancel_submitted(&offer_id, "m1", &cancel_tx, None)
        .expect("prepare keeps watches");
    assert_eq!(
        store
            .list_offer_ids_for_watched_coin(&coin)
            .expect("still watched"),
        vec![offer_id.clone()]
    );
    apply_watch_hits_batch(&store, std::slice::from_ref(&coin)).expect("hit");
    let rows = store
        .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
        .expect("rows");
    assert_eq!(rows[0].state, "cancel_submitted");
    store
        .ingest_tx_signals(std::slice::from_ref(&cancel_tx), TxSignalIngress::Mempool)
        .expect("observe");
    // Observe keeps watches; policy still ignores pure watch hits.
    assert_eq!(
        store
            .list_offer_ids_for_watched_coin(&coin)
            .expect("watches kept"),
        vec![offer_id.clone()]
    );
    apply_watch_hits_batch(&store, std::slice::from_ref(&coin)).expect("post-observe hit");
    let rows = store
        .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
        .expect("rows after observe hit");
    assert_eq!(rows[0].state, "cancel_submitted");
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

#[test]
fn cancel_tx_confirmation_promotes_cancel_submitted_to_cancelled() {
    let (_dir, store) = open_store();
    let offer_id = "ab".repeat(32);
    let cancel_tx = "cd".repeat(32);
    store
        .upsert_offer_cancel_submitted(&offer_id, "m1", &cancel_tx, None)
        .expect("cancel_submitted");
    store
        .ingest_tx_signals(std::slice::from_ref(&cancel_tx), TxSignalIngress::Confirmed)
        .expect("ingest");
    promote_cancel_submitted_for_confirmed_txs(&store, std::slice::from_ref(&cancel_tx))
        .expect("promote");
    let rows = store
        .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
        .expect("rows");
    assert_eq!(rows[0].state, "cancelled");
    let signal = store
        .get_tx_signal_state(std::slice::from_ref(&cancel_tx))
        .expect("signal");
    assert!(signal[&cancel_tx].tx_block_confirmed_at.is_some());
}
