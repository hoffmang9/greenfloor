use serde_json::{json, Value};

use crate::coinset::get_all_mempool_tx_ids;
use crate::config::ManagerProgramConfig;
use crate::daemon::coinset_tx::{classify_ws_payload, ClassifiedWsPayload};
use crate::daemon::coinset_ws::lifecycle::{apply_watch_hit_mempool, apply_ws_offer_event};
use crate::daemon::inventory_freshness::InventoryFreshnessCache;
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::operator_log::{
    LogContext, COINSET_WS_MEMPOOL_EVENT, COINSET_WS_PAYLOAD_IGNORED,
    COINSET_WS_PAYLOAD_PARSE_ERROR, COINSET_WS_RECOVERY_POLL, COINSET_WS_RECOVERY_POLL_ERROR,
    COINSET_WS_TX_BLOCK_EVENT, COIN_WATCH_HIT, MEMPOOL_OBSERVED, TX_BLOCK_CONFIRMED,
};
use crate::storage::SqliteStore;

pub async fn run_recovery_poll(
    store: &SqliteStore,
    program: &ManagerProgramConfig,
    coinset_base_url: &str,
    reason: &str,
) -> SignerResult<()> {
    let base_url = coinset_base_url.trim();
    let base_opt = if base_url.is_empty() {
        None
    } else {
        Some(base_url)
    };
    match get_all_mempool_tx_ids(&program.network, base_opt).await {
        Ok(tx_ids) => {
            let new_count = store.observe_mempool_tx_ids(&tx_ids)?;
            LogContext::COINSET.audit(
                store,
                COINSET_WS_RECOVERY_POLL,
                &json!({"reason": reason, "tx_id_count": tx_ids.len()}),
                None,
            )?;
            if new_count > 0 {
                LogContext::COINSET.audit(
                    store,
                    MEMPOOL_OBSERVED,
                    &json!({"new_tx_ids": new_count, "source": "coinset_websocket"}),
                    None,
                )?;
            }
            Ok(())
        }
        Err(err) => {
            LogContext::COINSET.audit(
                store,
                COINSET_WS_RECOVERY_POLL_ERROR,
                &json!({"reason": reason, "error": err.to_string()}),
                None,
            )?;
            Err(err)
        }
    }
}

fn record_ws_mempool_tx_ids(store: &SqliteStore, mempool_tx_ids: &[String]) -> SignerResult<()> {
    let new_count = store.observe_mempool_tx_ids(mempool_tx_ids)?;
    LogContext::COINSET.audit(
        store,
        COINSET_WS_MEMPOOL_EVENT,
        &json!({"tx_id_count": mempool_tx_ids.len()}),
        None,
    )?;
    if new_count > 0 {
        LogContext::COINSET.audit(
            store,
            MEMPOOL_OBSERVED,
            &json!({"new_tx_ids": new_count, "source": "coinset_websocket"}),
            None,
        )?;
    }
    Ok(())
}

fn record_ws_confirmed_tx_ids(
    store: &SqliteStore,
    confirmed_tx_ids: &[String],
) -> SignerResult<()> {
    let confirmed = store.confirm_tx_ids(confirmed_tx_ids)?;
    LogContext::COINSET.audit(
        store,
        COINSET_WS_TX_BLOCK_EVENT,
        &json!({"tx_id_count": confirmed_tx_ids.len(), "confirmed_count": confirmed}),
        None,
    )?;
    LogContext::COINSET.audit(
        store,
        TX_BLOCK_CONFIRMED,
        &json!({
            "tx_ids": confirmed_tx_ids,
            "confirmed_count": confirmed,
            "source": "coinset_websocket",
        }),
        None,
    )
}

fn record_ws_observed_p2s(
    store: &SqliteStore,
    inventory_freshness: &InventoryFreshnessCache,
    observed_p2s: &[String],
) -> SignerResult<()> {
    let market_ids = store.list_market_ids_for_watched_keys(observed_p2s)?;
    if market_ids.is_empty() {
        return Ok(());
    }
    for market_id in &market_ids {
        inventory_freshness.mark_stale(market_id);
    }
    for p2 in observed_p2s {
        apply_watch_hit_mempool(store, p2)?;
    }
    let mut sample: Vec<String> = observed_p2s
        .iter()
        .map(|key| normalize_hex_id(key))
        .collect();
    sample.sort();
    sample.truncate(10);
    LogContext::COINSET.audit(
        store,
        COIN_WATCH_HIT,
        &json!({
            "p2_count": observed_p2s.len(),
            "p2s_sample": sample,
            "market_ids": market_ids,
            "source": "coinset_websocket",
        }),
        None,
    )
}

fn record_ws_offer_events(
    store: &SqliteStore,
    classified: &ClassifiedWsPayload,
) -> SignerResult<()> {
    for event in &classified.offer_events {
        LogContext::COINSET.audit(
            store,
            "coinset_ws_offer_event",
            &json!({
                "offer_id": event.offer_id,
                "status": event.status,
                "tx_id": event.tx_id,
                "p2_count": event.p2s.len(),
                "source": "coinset_websocket",
            }),
            None,
        )?;
        apply_ws_offer_event(store, event)?;
    }
    Ok(())
}

pub async fn handle_ws_text(
    store: &SqliteStore,
    inventory_freshness: &InventoryFreshnessCache,
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
    let classified = classify_ws_payload(&payload);
    if !classified.mempool_tx_ids.is_empty() {
        record_ws_mempool_tx_ids(store, &classified.mempool_tx_ids)?;
    }
    if !classified.confirmed_tx_ids.is_empty() {
        record_ws_confirmed_tx_ids(store, &classified.confirmed_tx_ids)?;
    }
    record_ws_offer_events(store, &classified)?;
    if !classified.observed_p2s.is_empty() {
        record_ws_observed_p2s(store, inventory_freshness, &classified.observed_p2s)?;
    }
    Ok(())
}

pub fn ws_error(err: &tokio_tungstenite::tungstenite::Error) -> SignerError {
    SignerError::Other(format!("coinset_ws_once_error:{err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::inventory_freshness::InventoryFreshnessCache;
    use tempfile::tempdir;

    fn open_store() -> (tempfile::TempDir, SqliteStore) {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("state.db");
        let store = SqliteStore::open(&path).expect("open");
        (dir, store)
    }

    #[tokio::test]
    async fn handle_ws_text_routes_envelope_transaction() {
        let (_dir, store) = open_store();
        let freshness = InventoryFreshnessCache::new();
        let tx_id = "ab".repeat(32);
        handle_ws_text(
            &store,
            &freshness,
            &json!({
                "message": {
                    "type": "transaction",
                    "data": {"status": "pending", "ids": [tx_id]}
                }
            })
            .to_string(),
        )
        .await
        .expect("envelope");
        let events = store
            .list_recent_audit_events(Some(&[COINSET_WS_MEMPOOL_EVENT]), None, 5)
            .expect("events");
        assert_eq!(events.len(), 1);
    }

    #[tokio::test]
    async fn handle_ws_text_offer_pending_drives_lifecycle() {
        let (_dir, store) = open_store();
        let freshness = InventoryFreshnessCache::new();
        let offer_id = "ab".repeat(32);
        store
            .upsert_offer_state(&offer_id, "m1", "open", None)
            .expect("upsert");
        handle_ws_text(
            &store,
            &freshness,
            &json!({
                "message": {
                    "type": "offer",
                    "data": {
                        "offer_id": offer_id,
                        "status": "pending",
                        "tx_id": "cd".repeat(32)
                    }
                }
            })
            .to_string(),
        )
        .await
        .expect("offer");
        let rows = store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows");
        assert_eq!(rows[0].state, "mempool_observed");
        let mempool = store
            .list_recent_audit_events(Some(&[COINSET_WS_MEMPOOL_EVENT]), None, 5)
            .expect("events");
        assert!(mempool.is_empty(), "offer frames must not seed tx buckets");
    }

    #[tokio::test]
    async fn handle_ws_text_watch_hit_drives_mempool_observed() {
        let (_dir, store) = open_store();
        let freshness = InventoryFreshnessCache::new();
        let offer_id = "ab".repeat(32);
        let p2 = "ef".repeat(32);
        store
            .upsert_offer_state(&offer_id, "m1", "open", None)
            .expect("upsert");
        store
            .replace_offer_coin_watches(&offer_id, "m1", &[], std::slice::from_ref(&p2))
            .expect("watch");
        handle_ws_text(
            &store,
            &freshness,
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
        .await
        .expect("hit");
        let rows = store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows");
        assert_eq!(rows[0].state, "mempool_observed");
        assert!(freshness.needs_refresh("m1", std::time::Duration::from_secs(90)));
    }

    #[tokio::test]
    async fn handle_ws_text_emits_parse_error_for_invalid_json() {
        let (_dir, store) = open_store();
        let freshness = InventoryFreshnessCache::new();
        handle_ws_text(&store, &freshness, "{not-json")
            .await
            .expect("parse error audit");
        let events = store
            .list_recent_audit_events(Some(&[COINSET_WS_PAYLOAD_PARSE_ERROR]), None, 5)
            .expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, COINSET_WS_PAYLOAD_PARSE_ERROR);
    }

    #[tokio::test]
    async fn non_envelope_payload_is_ignored_without_mempool_audit() {
        let (_dir, store) = open_store();
        let freshness = InventoryFreshnessCache::new();
        let tx_id = "c".repeat(64);
        handle_ws_text(
            &store,
            &freshness,
            &json!({"event": "mempool_seen", "tx_id": tx_id}).to_string(),
        )
        .await
        .expect("ignored");
        let events = store
            .list_recent_audit_events(Some(&[COINSET_WS_MEMPOOL_EVENT]), None, 5)
            .expect("events");
        assert!(events.is_empty());
    }
}
