use crate::common::open_store;

#[test]
fn upsert_offer_cancel_submitted_seeds_tx_signal_state() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("greenfloor.sqlite"));
    let tx_id = "f".repeat(64);
    store
        .upsert_offer_cancel_submitted("offer-1", "m1", &tx_id, Some(0))
        .expect("cancel submitted");
    let state = store
        .get_tx_signal_state(std::slice::from_ref(&tx_id))
        .expect("state");
    assert!(state
        .get(&tx_id)
        .is_some_and(|row| row.mempool_observed_at.is_some()));
    assert_eq!(
        store
            .list_offer_states_for_ids(&["offer-1".to_string()])
            .expect("offer state")
            .first()
            .map(|row| row.state.as_str()),
        Some("cancel_submitted")
    );
}

#[test]
fn cancel_tracking_columns_cleared_on_open_upsert() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("greenfloor.sqlite"));
    let tx_id = "e".repeat(64);
    store
        .upsert_offer_cancel_submitted("offer-1", "m1", &tx_id, Some(0))
        .expect("seed cancel submitted");
    store
        .upsert_offer_state("offer-1", "m1", "open", Some(0))
        .expect("open reconcile");
    let row = store
        .list_offer_states_for_ids(&["offer-1".to_string()])
        .expect("offer state")
        .into_iter()
        .next()
        .expect("row");
    assert_eq!(row.state, "open");
    assert!(row.cancel_submitted_tx_id.is_none());
    assert!(row.cancel_submitted_at.is_none());
}

#[test]
fn cancel_submitted_at_survives_reconcile_preserve_upsert() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("greenfloor.sqlite"));
    let tx_id = "c".repeat(64);
    store
        .upsert_offer_cancel_submitted("offer-1", "m1", &tx_id, Some(0))
        .expect("initial cancel submitted");
    let submitted_at = store
        .list_offer_states_for_ids(&["offer-1".to_string()])
        .expect("offer state")
        .into_iter()
        .next()
        .and_then(|row| row.cancel_submitted_at);
    assert!(
        submitted_at.is_some(),
        "cancel submit should record timestamp"
    );
    store
        .upsert_offer_state_at(
            "offer-1",
            "m1",
            "cancel_submitted",
            Some(0),
            "2020-01-01T00:10:00Z",
        )
        .expect("preserve reconcile");
    let row = store
        .list_offer_states_for_ids(&["offer-1".to_string()])
        .expect("offer state")
        .into_iter()
        .next()
        .expect("row");
    assert_eq!(row.updated_at, "2020-01-01T00:10:00Z");
    assert_eq!(row.cancel_submitted_at, submitted_at);
}

#[test]
fn upsert_offer_state_insert_and_list() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    store
        .upsert_offer_state("offer-1", "m1", "open", Some(4))
        .expect("upsert");
    let rows = store.list_offer_states(None, 10).expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].offer_id, "offer-1");
    assert_eq!(rows[0].market_id, "m1");
    assert_eq!(rows[0].state, "open");
    assert_eq!(rows[0].last_seen_status, Some(4));
}

#[test]
fn upsert_offer_state_updates_existing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    store
        .upsert_offer_state("offer-1", "m1", "open", Some(4))
        .expect("upsert open");
    store
        .upsert_offer_state("offer-1", "m1", "expired", Some(6))
        .expect("upsert expired");
    let rows = store.list_offer_states(None, 10).expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, "expired");
    assert_eq!(rows[0].last_seen_status, Some(6));
}

#[test]
fn upsert_offer_state_null_last_seen_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    store
        .upsert_offer_state("offer-2", "m1", "unknown", None)
        .expect("upsert");
    let rows = store.list_offer_states(None, 10).expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].last_seen_status, None);
}

#[test]
fn list_offer_states_filters_by_market_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    store
        .upsert_offer_state("o1", "m1", "open", Some(4))
        .expect("o1");
    store
        .upsert_offer_state("o2", "m2", "open", Some(4))
        .expect("o2");
    store
        .upsert_offer_state("o3", "m1", "expired", Some(6))
        .expect("o3");
    let m1_rows = store.list_offer_states(Some("m1"), 10).expect("m1");
    assert_eq!(m1_rows.len(), 2);
    assert!(m1_rows.iter().all(|row| row.market_id == "m1"));
    assert_eq!(
        store.list_offer_states(Some("m2"), 10).expect("m2").len(),
        1
    );
}

#[test]
fn list_offer_states_respects_limit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    for i in 0..5 {
        store
            .upsert_offer_state(&format!("o{i}"), "m1", "open", Some(4))
            .expect("seed");
    }
    assert_eq!(store.list_offer_states(None, 3).expect("limit 3").len(), 3);
    assert!(store
        .list_offer_states(None, 0)
        .expect("limit 0")
        .is_empty());
}

#[test]
fn list_offer_states_orders_by_updated_at_desc() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    store
        .upsert_offer_state_at("first", "m1", "open", Some(4), "2020-01-01T00:00:00Z")
        .expect("first");
    store
        .upsert_offer_state_at("second", "m1", "open", Some(4), "2021-01-01T00:00:00Z")
        .expect("second");
    let rows = store.list_offer_states(None, 10).expect("list");
    assert_eq!(rows[0].offer_id, "second");
    assert_eq!(rows[1].offer_id, "first");
}

#[test]
fn upsert_offer_reconcile_state_persists_typed_state() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("state.db"));
    store
        .upsert_offer_reconcile_state(
            "offer-rc",
            "m1",
            &greenfloor_engine::cycle::reconcile::ReconcileState::Lifecycle(
                greenfloor_engine::cycle::OfferLifecycleState::Open,
            ),
            Some(1),
        )
        .expect("upsert");
    assert_eq!(
        store
            .list_offer_states_for_ids(&["offer-rc".to_string()])
            .expect("lookup")
            .into_iter()
            .next()
            .expect("row")
            .state,
        "open"
    );
}

#[test]
fn list_open_offer_states_page_and_all_open_include_pending_visibility() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("state.db"));
    store
        .upsert_offer_state("open-offer", "m1", "open", Some(0))
        .expect("open");
    store
        .upsert_offer_state("pending-offer", "m1", "pending_visibility", Some(0))
        .expect("pending");
    store
        .upsert_offer_state("expired-offer", "m1", "expired", Some(0))
        .expect("expired");
    let page = store.list_open_offer_states_page(10, 0).expect("page");
    assert_eq!(page.len(), 2);
    let all = store.list_all_open_offer_states().expect("all open");
    assert_eq!(all.len(), 2);
    assert!(all
        .iter()
        .any(|row| row.offer_id == "open-offer" && row.state == "open"));
    assert!(all.iter().any(|row| row.offer_id == "pending-offer"));
    assert!(store
        .list_open_offer_states_page(0, 0)
        .expect("zero limit")
        .is_empty());
}

#[test]
fn list_offer_state_details_surfaces_cancel_submitted_state() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("state.db"));
    let tx_id = "d".repeat(64);
    store
        .upsert_offer_cancel_submitted("offer-cancel", "m1", &tx_id, Some(0))
        .expect("cancel submitted");
    let details = store.list_offer_state_details("m1", 10).expect("details");
    assert_eq!(details.len(), 1);
    assert_eq!(details[0].offer_id, "offer-cancel");
    assert_eq!(details[0].state, "cancel_submitted");
    assert!(store
        .list_offer_state_details("m1", 0)
        .expect("zero limit")
        .is_empty());
    let tracked = store
        .list_offer_states_for_ids(&["offer-cancel".to_string()])
        .expect("tracked cancel columns")
        .into_iter()
        .next()
        .expect("cancel row");
    assert_eq!(
        tracked.cancel_submitted_tx_id.as_deref(),
        Some(tx_id.as_str())
    );
    assert!(tracked.cancel_submitted_at.is_some());
}

#[test]
fn list_offer_states_for_ids_returns_empty_for_missing_offer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("state.db"));
    assert!(store
        .list_offer_states_for_ids(&["missing-offer".to_string()])
        .expect("lookup")
        .is_empty());
}

#[test]
fn list_offer_states_for_ids_returns_matches_in_input_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("state.db"));
    store
        .upsert_offer_state("offer-a", "m1", "open", Some(0))
        .expect("seed");
    store
        .upsert_offer_state("offer-b", "m1", "expired", Some(0))
        .expect("seed");
    store
        .upsert_offer_state("offer-c", "m2", "open", Some(0))
        .expect("seed");
    let rows = store
        .list_offer_states_for_ids(&[
            "offer-c".to_string(),
            "missing-offer".to_string(),
            "offer-a".to_string(),
            "offer-b".to_string(),
        ])
        .expect("rows");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].offer_id, "offer-c");
    assert_eq!(rows[1].offer_id, "offer-a");
    assert_eq!(rows[2].offer_id, "offer-b");
}
