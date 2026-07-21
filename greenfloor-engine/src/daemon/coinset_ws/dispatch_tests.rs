use super::*;
use crate::daemon::coinset_ws::InventoryP2Index;
use crate::operator_log::{
    COINSET_WS_MEMPOOL_EVENT, COINSET_WS_PAYLOAD_PARSE_ERROR, COIN_WATCH_HIT,
};
use tempfile::tempdir;

fn open_store() -> (tempfile::TempDir, SqliteStore) {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("state.db");
    let store = SqliteStore::open(&path).expect("open");
    (dir, store)
}

fn context_with_market_p2(p2: &str) -> std::sync::Arc<CoinsetWsShared> {
    let index = InventoryP2Index::from_markets_by_p2(std::collections::HashMap::from([(
        p2.to_string(),
        vec!["m1".to_string()],
    )]));
    CoinsetWsShared::new(index, crate::daemon::InventoryFreshnessCache::new())
}

#[test]
fn handle_ws_text_routes_envelope_transaction() {
    let (_dir, store) = open_store();
    let tx_id = "ab".repeat(32);
    handle_ws_text(
        &store,
        &CoinsetWsShared::empty(),
        &json!({
            "message": {
                "type": "transaction",
                "data": {"status": "pending", "ids": [tx_id]}
            }
        })
        .to_string(),
    )
    .expect("envelope");
    let events = store
        .list_recent_audit_events(Some(&[COINSET_WS_MEMPOOL_EVENT]), None, 5)
        .expect("events");
    assert_eq!(events.len(), 1);
}

#[test]
fn handle_ws_text_inventory_p2_marks_stale_without_offer_watch() {
    let (_dir, store) = open_store();
    let p2 = "ef".repeat(32);
    let ctx = context_with_market_p2(&p2);
    ctx.inventory_freshness
        .mark_fresh("m1", std::collections::BTreeMap::from([(50, 1)]));
    handle_ws_text(
        &store,
        &ctx,
        &json!({
            "message": {
                "type": "transaction",
                "data": {
                    "status": "pending",
                    "ids": ["cd".repeat(32)],
                    "p2s": [p2]
                }
            }
        })
        .to_string(),
    )
    .expect("hit");
    assert!(ctx
        .inventory_freshness
        .needs_refresh("m1", std::time::Duration::from_secs(90)));
}

#[test]
fn handle_ws_text_offer_with_p2s_does_not_drive_watch_or_inventory() {
    let (_dir, store) = open_store();
    let p2 = "ef".repeat(32);
    let ctx = context_with_market_p2(&p2);
    ctx.inventory_freshness
        .mark_fresh("m1", std::collections::BTreeMap::from([(50, 1)]));
    let offer_id = "ab".repeat(32);
    let watched_offer = "11".repeat(32);
    store
        .upsert_offer_state(&offer_id, "m1", "open", None)
        .expect("upsert offer");
    store
        .upsert_offer_state(&watched_offer, "m1", "open", None)
        .expect("upsert watched");
    store
        .replace_offer_coin_watches(&watched_offer, "m1", &[], std::slice::from_ref(&p2))
        .expect("watch");
    handle_ws_text(
        &store,
        &ctx,
        &json!({
            "message": {
                "type": "offer",
                "data": {
                    "offer_id": offer_id,
                    "status": "pending",
                    "tx_id": "cd".repeat(32),
                    "p2s": [p2]
                }
            }
        })
        .to_string(),
    )
    .expect("offer");
    assert!(!ctx
        .inventory_freshness
        .needs_refresh("m1", std::time::Duration::from_secs(90)));
    assert_eq!(
        store
            .list_offer_states_for_ids(std::slice::from_ref(&watched_offer))
            .expect("watched rows")[0]
            .state,
        "open"
    );
    assert!(store
        .list_recent_audit_events(Some(&[COIN_WATCH_HIT]), None, 5)
        .expect("watch audits")
        .is_empty());
    assert!(store
        .list_recent_audit_events(Some(&[COINSET_WS_MEMPOOL_EVENT]), None, 5)
        .expect("events")
        .is_empty());
    assert_eq!(
        store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("offer rows")[0]
            .state,
        "open"
    );
    let pending_tx = "cd".repeat(32);
    assert!(store
        .get_tx_signal_state(std::slice::from_ref(&pending_tx))
        .expect("signal")[&pending_tx]
        .mempool_observed_at
        .is_some());
}

#[test]
fn handle_ws_text_offer_confirmed_marks_inventory_stale() {
    let (_dir, store) = open_store();
    let ctx = CoinsetWsShared::new(
        std::sync::Arc::new(InventoryP2Index::default()),
        crate::daemon::InventoryFreshnessCache::new(),
    );
    ctx.inventory_freshness
        .mark_fresh("m1", std::collections::BTreeMap::from([(50, 1)]));
    let offer_id = "ab".repeat(32);
    store
        .upsert_offer_state(&offer_id, "m1", "open", None)
        .expect("upsert");
    handle_ws_text(
        &store,
        &ctx,
        &json!({
            "message": {
                "type": "offer",
                "data": {
                    "offer_id": offer_id,
                    "status": "confirmed",
                    "tx_id": "cd".repeat(32),
                }
            }
        })
        .to_string(),
    )
    .expect("offer");
    assert!(ctx
        .inventory_freshness
        .needs_refresh("m1", std::time::Duration::from_secs(90)));
}

#[test]
fn handle_ws_text_maker_watch_hit_marks_inventory_without_inventory_p2() {
    let (_dir, store) = open_store();
    let maker_p2 = "ef".repeat(32);
    let ctx = CoinsetWsShared::new(
        std::sync::Arc::new(InventoryP2Index::default()),
        crate::daemon::InventoryFreshnessCache::new(),
    );
    ctx.inventory_freshness
        .mark_fresh("m1", std::collections::BTreeMap::from([(50, 1)]));
    let offer_id = "ab".repeat(32);
    store
        .upsert_offer_state(&offer_id, "m1", "open", None)
        .expect("upsert");
    store
        .replace_offer_coin_watches(&offer_id, "m1", &[], std::slice::from_ref(&maker_p2))
        .expect("watch");
    handle_ws_text(
        &store,
        &ctx,
        &json!({
            "message": {
                "type": "transaction",
                "data": {
                    "status": "pending",
                    "ids": ["cd".repeat(32)],
                    "p2s": [maker_p2]
                }
            }
        })
        .to_string(),
    )
    .expect("tx");
    assert!(ctx
        .inventory_freshness
        .needs_refresh("m1", std::time::Duration::from_secs(90)));
    assert_eq!(
        store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows")[0]
            .state,
        "open"
    );
}

#[test]
fn handle_ws_text_unknown_tx_status_marks_inventory_without_lifecycle() {
    let (_dir, store) = open_store();
    let maker_p2 = "ef".repeat(32);
    let ctx = CoinsetWsShared::new(
        std::sync::Arc::new(InventoryP2Index::default()),
        crate::daemon::InventoryFreshnessCache::new(),
    );
    ctx.inventory_freshness
        .mark_fresh("m1", std::collections::BTreeMap::from([(50, 1)]));
    let offer_id = "ab".repeat(32);
    store
        .upsert_offer_state(&offer_id, "m1", "open", None)
        .expect("upsert");
    store
        .replace_offer_coin_watches(&offer_id, "m1", &[], std::slice::from_ref(&maker_p2))
        .expect("watch");
    handle_ws_text(
        &store,
        &ctx,
        &json!({
            "message": {
                "type": "transaction",
                "data": {
                    "status": "unknown",
                    "ids": ["cd".repeat(32)],
                    "p2s": [maker_p2]
                }
            }
        })
        .to_string(),
    )
    .expect("tx");
    assert!(ctx
        .inventory_freshness
        .needs_refresh("m1", std::time::Duration::from_secs(90)));
    assert_eq!(
        store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows")[0]
            .state,
        "open"
    );
}

#[test]
fn handle_ws_text_confirmed_maker_coin_watch_promotes_lifecycle() {
    let (_dir, store) = open_store();
    let maker_coin = "ef".repeat(32);
    let ctx = CoinsetWsShared::new(
        std::sync::Arc::new(InventoryP2Index::default()),
        crate::daemon::InventoryFreshnessCache::new(),
    );
    ctx.inventory_freshness
        .mark_fresh("m1", std::collections::BTreeMap::from([(50, 1)]));
    let offer_id = "ab".repeat(32);
    store
        .upsert_offer_state(&offer_id, "m1", "open", None)
        .expect("upsert");
    store
        .replace_offer_coin_watches(&offer_id, "m1", std::slice::from_ref(&maker_coin), &[])
        .expect("watch");
    handle_ws_text(
        &store,
        &ctx,
        &json!({
            "message": {
                "type": "transaction",
                "data": {
                    "status": "confirmed",
                    "ids": ["cd".repeat(32)],
                    "coin_ids": [maker_coin]
                }
            }
        })
        .to_string(),
    )
    .expect("tx");
    assert!(ctx
        .inventory_freshness
        .needs_refresh("m1", std::time::Duration::from_secs(90)));
    assert_eq!(
        store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows")[0]
            .state,
        "tx_block_confirmed"
    );
}

#[test]
fn handle_ws_text_p2_only_watch_keeps_open_and_marks_inventory_stale() {
    let (_dir, store) = open_store();
    let maker_p2 = "ef".repeat(32);
    let ctx = CoinsetWsShared::new(
        std::sync::Arc::new(InventoryP2Index::default()),
        crate::daemon::InventoryFreshnessCache::new(),
    );
    ctx.inventory_freshness
        .mark_fresh("m1", std::collections::BTreeMap::from([(50, 1)]));
    let offer_id = "ab".repeat(32);
    store
        .upsert_offer_state(&offer_id, "m1", "open", None)
        .expect("upsert");
    store
        .replace_offer_coin_watches(&offer_id, "m1", &[], std::slice::from_ref(&maker_p2))
        .expect("watch");
    handle_ws_text(
        &store,
        &ctx,
        &json!({
            "message": {
                "type": "transaction",
                "data": {
                    "status": "confirmed",
                    "ids": ["cd".repeat(32)],
                    "p2s": [maker_p2]
                }
            }
        })
        .to_string(),
    )
    .expect("tx");
    assert!(ctx
        .inventory_freshness
        .needs_refresh("m1", std::time::Duration::from_secs(90)));
    assert_eq!(
        store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows")[0]
            .state,
        "open"
    );
}

#[test]
fn handle_ws_text_emits_parse_error_for_invalid_json() {
    let (_dir, store) = open_store();
    handle_ws_text(&store, &CoinsetWsShared::empty(), "{not-json").expect("parse error audit");
    assert_eq!(
        store
            .list_recent_audit_events(Some(&[COINSET_WS_PAYLOAD_PARSE_ERROR]), None, 5)
            .expect("events")
            .len(),
        1
    );
}

#[test]
fn non_envelope_payload_is_ignored_without_mempool_audit() {
    let (_dir, store) = open_store();
    handle_ws_text(
        &store,
        &CoinsetWsShared::empty(),
        &json!({"event": "mempool_seen", "tx_id": "c".repeat(64)}).to_string(),
    )
    .expect("ignored");
    assert!(store
        .list_recent_audit_events(Some(&[COINSET_WS_MEMPOOL_EVENT]), None, 5)
        .expect("events")
        .is_empty());
}

#[tokio::test]
async fn http_confirm_promotes_cancel_submitted_offer() {
    let (_dir, store) = open_store();
    let offer_id = "ab".repeat(32);
    let cancel_tx = "cd".repeat(32);
    store
        .prepare_offer_cancel_submitted(&offer_id, "m1", &cancel_tx, None)
        .expect("prepare");
    let mut server = mockito::Server::new_async().await;
    let _transaction = server
        .mock("POST", "/get_transaction")
        .with_status(200)
        .with_body(r#"{"success":true,"state":"confirmed"}"#)
        .create();

    let markets = confirm_cancel_submitted_txs_via_http(&store, "mainnet", &server.url())
        .await
        .expect("confirm");

    assert_eq!(markets, vec!["m1".to_string()]);
    assert_eq!(
        store
            .offer_state_for_id(&offer_id)
            .expect("state")
            .as_deref(),
        Some("cancelled")
    );
}
