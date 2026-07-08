use serde_json::{json, Value};

use crate::coinset::get_all_mempool_tx_ids;
use crate::config::ManagerProgramConfig;
use crate::daemon::coinset_tx::{parse_ws_event, WsEvent};
use crate::daemon::coinset_ws::lifecycle::{apply_watch_hit_mempool, apply_ws_offer_event};
use crate::daemon::coinset_ws::InventoryP2Index;
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

fn record_observed_p2s(
    store: &SqliteStore,
    inventory_p2s: &InventoryP2Index,
    inventory_freshness: &InventoryFreshnessCache,
    observed_p2s: &[String],
) -> SignerResult<()> {
    let inventory_markets = inventory_p2s.market_ids_for_p2s(observed_p2s);
    for market_id in &inventory_markets {
        inventory_freshness.mark_stale(market_id);
    }
    let watch_markets = store.list_market_ids_for_watched_keys(observed_p2s)?;
    for p2 in observed_p2s {
        apply_watch_hit_mempool(store, p2)?;
    }
    if inventory_markets.is_empty() && watch_markets.is_empty() {
        return Ok(());
    }
    let mut sample: Vec<String> = observed_p2s
        .iter()
        .map(|key| normalize_hex_id(key))
        .collect();
    sample.sort();
    sample.truncate(10);
    let mut market_ids = inventory_markets;
    market_ids.extend(watch_markets);
    market_ids.sort();
    market_ids.dedup();
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

fn apply_ws_event(
    store: &SqliteStore,
    inventory_p2s: &InventoryP2Index,
    inventory_freshness: &InventoryFreshnessCache,
    event: WsEvent,
) -> SignerResult<()> {
    match event {
        WsEvent::Transaction(tx) => {
            match tx.status.as_str() {
                "pending" if !tx.tx_ids.is_empty() => {
                    record_ws_mempool_tx_ids(store, &tx.tx_ids)?;
                }
                "confirmed" if !tx.tx_ids.is_empty() => {
                    record_ws_confirmed_tx_ids(store, &tx.tx_ids)?;
                }
                _ => {}
            }
            if !tx.p2s.is_empty() {
                record_observed_p2s(store, inventory_p2s, inventory_freshness, &tx.p2s)?;
            }
        }
        WsEvent::Offer(offer) => {
            LogContext::COINSET.audit(
                store,
                "coinset_ws_offer_event",
                &json!({
                    "offer_id": offer.offer_id,
                    "status": offer.status,
                    "tx_id": offer.tx_id,
                    "p2_count": offer.p2s.len(),
                    "source": "coinset_websocket",
                }),
                None,
            )?;
            let p2s = offer.p2s.clone();
            apply_ws_offer_event(store, &offer)?;
            if !p2s.is_empty() {
                record_observed_p2s(store, inventory_p2s, inventory_freshness, &p2s)?;
            }
        }
    }
    Ok(())
}

pub fn handle_ws_text(
    store: &SqliteStore,
    inventory_p2s: &InventoryP2Index,
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
    let Some(event) = parse_ws_event(&payload) else {
        return Ok(());
    };
    apply_ws_event(store, inventory_p2s, inventory_freshness, event)
}

pub fn ws_error(err: &tokio_tungstenite::tungstenite::Error) -> SignerError {
    SignerError::Other(format!("coinset_ws_once_error:{err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::inventory_freshness::InventoryFreshnessCache;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn open_store() -> (tempfile::TempDir, SqliteStore) {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("state.db");
        let store = SqliteStore::open(&path).expect("open");
        (dir, store)
    }

    fn empty_index() -> Arc<InventoryP2Index> {
        Arc::new(InventoryP2Index::default())
    }

    #[test]
    fn handle_ws_text_routes_envelope_transaction() {
        let (_dir, store) = open_store();
        let freshness = InventoryFreshnessCache::new();
        let tx_id = "ab".repeat(32);
        handle_ws_text(
            &store,
            &empty_index(),
            &freshness,
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
    fn handle_ws_text_offer_pending_drives_lifecycle() {
        let (_dir, store) = open_store();
        let freshness = InventoryFreshnessCache::new();
        let offer_id = "ab".repeat(32);
        store
            .upsert_offer_state(&offer_id, "m1", "open", None)
            .expect("upsert");
        handle_ws_text(
            &store,
            &empty_index(),
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
        .expect("offer");
        let rows = store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows");
        assert_eq!(rows[0].state, "mempool_observed");
        let mempool = store
            .list_recent_audit_events(Some(&[COINSET_WS_MEMPOOL_EVENT]), None, 5)
            .expect("events");
        assert!(mempool.is_empty(), "offer frames must not seed tx buckets");
        let tx_id = "cd".repeat(32);
        let signals = store
            .get_tx_signal_state(std::slice::from_ref(&tx_id))
            .expect("tx signals");
        assert!(
            signals
                .get(&tx_id)
                .is_some_and(|row| row.mempool_observed_at.is_some()),
            "offer pending must seed tx_signal_state for later cancel/reconcile"
        );
    }

    #[test]
    fn handle_ws_text_inventory_p2_marks_stale_without_offer_watch() {
        let (_dir, store) = open_store();
        let freshness = InventoryFreshnessCache::new();
        freshness.mark_fresh("m1", std::collections::BTreeMap::from([(50, 1)]));
        let p2 = "ef".repeat(32);
        let mut markets_by_p2 = std::collections::HashMap::new();
        markets_by_p2.insert(p2.clone(), vec!["m1".to_string()]);
        let index = InventoryP2Index::from_markets_by_p2(markets_by_p2);
        handle_ws_text(
            &store,
            &index,
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
        .expect("hit");
        assert!(freshness.needs_refresh("m1", std::time::Duration::from_secs(90)));
    }

    #[test]
    fn handle_ws_text_watch_hit_drives_mempool_observed() {
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
            &empty_index(),
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
        .expect("hit");
        let rows = store
            .list_offer_states_for_ids(std::slice::from_ref(&offer_id))
            .expect("rows");
        assert_eq!(rows[0].state, "mempool_observed");
    }

    #[test]
    fn handle_ws_text_emits_parse_error_for_invalid_json() {
        let (_dir, store) = open_store();
        let freshness = InventoryFreshnessCache::new();
        handle_ws_text(&store, &empty_index(), &freshness, "{not-json").expect("parse error audit");
        let events = store
            .list_recent_audit_events(Some(&[COINSET_WS_PAYLOAD_PARSE_ERROR]), None, 5)
            .expect("events");
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn non_envelope_payload_is_ignored_without_mempool_audit() {
        let (_dir, store) = open_store();
        let freshness = InventoryFreshnessCache::new();
        let tx_id = "c".repeat(64);
        handle_ws_text(
            &store,
            &empty_index(),
            &freshness,
            &json!({"event": "mempool_seen", "tx_id": tx_id}).to_string(),
        )
        .expect("ignored");
        let events = store
            .list_recent_audit_events(Some(&[COINSET_WS_MEMPOOL_EVENT]), None, 5)
            .expect("events");
        assert!(events.is_empty());
    }
}
