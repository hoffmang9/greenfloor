use serde_json::{json, Value};

use crate::coinset::parse_ws_event;
use crate::daemon::coinset_ws::CoinsetProcessContext;
use crate::error::{SignerError, SignerResult};
use crate::operator_log::{LogContext, COINSET_WS_PAYLOAD_IGNORED, COINSET_WS_PAYLOAD_PARSE_ERROR};
use crate::storage::SqliteStore;

use super::dispatch::apply_ws_event;

pub use super::dispatch::run_recovery_poll;

pub fn handle_ws_text(
    store: &SqliteStore,
    ctx: &CoinsetProcessContext,
    raw: &str,
) -> SignerResult<()> {
    let payload: Value = if let Ok(value) = serde_json::from_str(raw) {
        value
    } else {
        LogContext::COINSET.audit(
            store,
            COINSET_WS_PAYLOAD_PARSE_ERROR,
            &json!({"raw": raw.chars().take(200).collect::<String>()}),
            None,
        )?;
        return Ok(());
    };
    if !payload.is_object() {
        let kind = match &payload {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        };
        LogContext::COINSET.audit(
            store,
            COINSET_WS_PAYLOAD_IGNORED,
            &json!({"kind": kind}),
            None,
        )?;
        return Ok(());
    }
    let Some(event) = parse_ws_event(&payload) else {
        return Ok(());
    };
    apply_ws_event(store, ctx, event)
}

pub fn ws_error(err: &tokio_tungstenite::tungstenite::Error) -> SignerError {
    SignerError::Other(format!("coinset_ws_once_error:{err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::coinset_ws::InventoryP2Index;
    use crate::operator_log::{COINSET_WS_MEMPOOL_EVENT, COIN_WATCH_HIT};
    use tempfile::tempdir;

    fn open_store() -> (tempfile::TempDir, SqliteStore) {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("state.db");
        let store = SqliteStore::open(&path).expect("open");
        (dir, store)
    }

    #[test]
    fn handle_ws_text_routes_envelope_transaction() {
        let (_dir, store) = open_store();
        let ctx = CoinsetProcessContext::empty();
        let tx_id = "ab".repeat(32);
        handle_ws_text(
            &store,
            &ctx,
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
        let mut markets_by_p2 = std::collections::HashMap::new();
        markets_by_p2.insert(p2.clone(), vec!["m1".to_string()]);
        let index = InventoryP2Index::from_markets_by_p2(markets_by_p2);
        let ctx = CoinsetProcessContext::new(index, crate::daemon::InventoryFreshnessCache::new());
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
        let mut markets_by_p2 = std::collections::HashMap::new();
        markets_by_p2.insert(p2.clone(), vec!["m1".to_string()]);
        let index = InventoryP2Index::from_markets_by_p2(markets_by_p2);
        let ctx = CoinsetProcessContext::new(index, crate::daemon::InventoryFreshnessCache::new());
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
        assert!(
            !ctx.inventory_freshness
                .needs_refresh("m1", std::time::Duration::from_secs(90)),
            "offer-frame p2s must not mark inventory stale"
        );
        let watched_rows = store
            .list_offer_states_for_ids(std::slice::from_ref(&watched_offer))
            .expect("watched rows");
        assert_eq!(
            watched_rows[0].state, "open",
            "offer-frame p2s must not apply watch-hit lifecycle"
        );
        let watch_audits = store
            .list_recent_audit_events(Some(&[COIN_WATCH_HIT]), None, 5)
            .expect("watch audits");
        assert!(
            watch_audits.is_empty(),
            "offer frames must not emit coin_watch_hit"
        );
        let mempool = store
            .list_recent_audit_events(Some(&[COINSET_WS_MEMPOOL_EVENT]), None, 5)
            .expect("events");
        assert!(
            mempool.is_empty(),
            "offer frames must not seed transaction mempool audits"
        );
        let offer_rows = store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("offer rows");
        assert_eq!(offer_rows[0].state, "mempool_observed");
    }

    #[test]
    fn handle_ws_text_emits_parse_error_for_invalid_json() {
        let (_dir, store) = open_store();
        let ctx = CoinsetProcessContext::empty();
        handle_ws_text(&store, &ctx, "{not-json").expect("parse error audit");
        let events = store
            .list_recent_audit_events(Some(&[COINSET_WS_PAYLOAD_PARSE_ERROR]), None, 5)
            .expect("events");
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn non_envelope_payload_is_ignored_without_mempool_audit() {
        let (_dir, store) = open_store();
        let ctx = CoinsetProcessContext::empty();
        let tx_id = "c".repeat(64);
        handle_ws_text(
            &store,
            &ctx,
            &json!({"event": "mempool_seen", "tx_id": tx_id}).to_string(),
        )
        .expect("ignored");
        let events = store
            .list_recent_audit_events(Some(&[COINSET_WS_MEMPOOL_EVENT]), None, 5)
            .expect("events");
        assert!(events.is_empty());
    }
}
