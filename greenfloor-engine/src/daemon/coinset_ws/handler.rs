use serde_json::{json, Value};

use crate::coinset::get_all_mempool_tx_ids;
use crate::config::ManagerProgramConfig;
use crate::daemon::coinset_tx::classify_ws_payload_tx_ids;
use crate::daemon::watchlist::cache::CoinWatchlistCache;
use crate::error::{SignerError, SignerResult};
use crate::offer::dexie_payload::{
    extract_coin_ids_from_offer_payload, extract_coinset_tx_ids_from_offer_payload,
};
use crate::operator_log::{
    audit_coinset, AuditDurability, COINSET_WS_COIN_OBSERVED, COINSET_WS_MEMPOOL_EVENT,
    COINSET_WS_PAYLOAD_IGNORED, COINSET_WS_PAYLOAD_PARSE_ERROR, COINSET_WS_RECOVERY_POLL,
    COINSET_WS_RECOVERY_POLL_ERROR, COINSET_WS_TX_BLOCK_EVENT, COIN_WATCH_HIT, MEMPOOL_OBSERVED,
    TX_BLOCK_CONFIRMED,
};
use crate::storage::SqliteStore;

fn coinset_audit(
    store: &SqliteStore,
    event_type: &str,
    payload: &Value,
    market_id: Option<&str>,
    durability: AuditDurability,
) -> SignerResult<()> {
    audit_coinset(store, event_type, payload, market_id, durability)
}

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
            coinset_audit(
                store,
                COINSET_WS_RECOVERY_POLL,
                &json!({"reason": reason, "tx_id_count": tx_ids.len()}),
                None,
                AuditDurability::Required,
            )?;
            if new_count > 0 {
                coinset_audit(
                    store,
                    MEMPOOL_OBSERVED,
                    &json!({"new_tx_ids": new_count, "source": "coinset_websocket"}),
                    None,
                    AuditDurability::Required,
                )?;
            }
            Ok(())
        }
        Err(err) => {
            coinset_audit(
                store,
                COINSET_WS_RECOVERY_POLL_ERROR,
                &json!({"reason": reason, "error": err.to_string()}),
                None,
                AuditDurability::Required,
            )?;
            Err(err)
        }
    }
}

fn record_ws_mempool_tx_ids(store: &SqliteStore, mempool_tx_ids: &[String]) -> SignerResult<()> {
    let new_count = store.observe_mempool_tx_ids(mempool_tx_ids)?;
    coinset_audit(
        store,
        COINSET_WS_MEMPOOL_EVENT,
        &json!({"tx_id_count": mempool_tx_ids.len()}),
        None,
        AuditDurability::Required,
    )?;
    if new_count > 0 {
        coinset_audit(
            store,
            MEMPOOL_OBSERVED,
            &json!({"new_tx_ids": new_count, "source": "coinset_websocket"}),
            None,
            AuditDurability::Required,
        )?;
    }
    Ok(())
}

fn record_ws_confirmed_tx_ids(
    store: &SqliteStore,
    confirmed_tx_ids: &[String],
) -> SignerResult<()> {
    let confirmed = store.confirm_tx_ids(confirmed_tx_ids)?;
    coinset_audit(
        store,
        COINSET_WS_TX_BLOCK_EVENT,
        &json!({"tx_id_count": confirmed_tx_ids.len(), "confirmed_count": confirmed}),
        None,
        AuditDurability::Required,
    )?;
    coinset_audit(
        store,
        TX_BLOCK_CONFIRMED,
        &json!({
            "tx_ids": confirmed_tx_ids,
            "confirmed_count": confirmed,
            "source": "coinset_websocket",
        }),
        None,
        AuditDurability::Required,
    )
}

fn record_ws_observed_coins(
    store: &SqliteStore,
    coin_watchlist: &CoinWatchlistCache,
    observed_coin_ids: &[String],
) -> SignerResult<()> {
    coinset_audit(
        store,
        COINSET_WS_COIN_OBSERVED,
        &json!({"coin_id_count": observed_coin_ids.len()}),
        None,
        AuditDurability::Required,
    )?;
    let hits = coin_watchlist.match_watched_coin_ids(observed_coin_ids);
    if hits.is_empty() {
        return Ok(());
    }
    let mut sample: Vec<String> = observed_coin_ids
        .iter()
        .map(|coin_id| coin_id.trim().to_ascii_lowercase())
        .collect();
    sample.sort();
    sample.truncate(10);
    let market_hits: serde_json::Map<String, Value> = hits
        .into_iter()
        .map(|(market_id, coin_ids)| {
            (
                market_id,
                Value::Array(coin_ids.into_iter().take(10).map(Value::String).collect()),
            )
        })
        .collect();
    coinset_audit(
        store,
        COIN_WATCH_HIT,
        &json!({
            "coin_id_count": observed_coin_ids.len(),
            "coin_ids_sample": sample,
            "market_hits": market_hits,
            "source": "coinset_websocket",
        }),
        None,
        AuditDurability::Required,
    )
}

pub fn handle_ws_text(
    store: &SqliteStore,
    coin_watchlist: &CoinWatchlistCache,
    raw: &str,
) -> SignerResult<()> {
    let payload: Value = if let Ok(value) = serde_json::from_str(raw) {
        value
    } else {
        coinset_audit(
            store,
            COINSET_WS_PAYLOAD_PARSE_ERROR,
            &json!({"raw": raw.chars().take(200).collect::<String>()}),
            None,
            AuditDurability::Required,
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
        coinset_audit(
            store,
            COINSET_WS_PAYLOAD_IGNORED,
            &json!({"kind": kind}),
            None,
            AuditDurability::Required,
        )?;
        return Ok(());
    }
    let (mempool_tx_ids, confirmed_tx_ids) = classify_ws_payload_tx_ids(&payload);
    if !mempool_tx_ids.is_empty() {
        record_ws_mempool_tx_ids(store, &mempool_tx_ids)?;
    }
    if !confirmed_tx_ids.is_empty() {
        record_ws_confirmed_tx_ids(store, &confirmed_tx_ids)?;
    }
    let observed_coin_ids = extract_coin_ids_from_offer_payload(&payload);
    if !observed_coin_ids.is_empty() {
        record_ws_observed_coins(store, coin_watchlist, &observed_coin_ids)?;
    }
    let _coinset_tx_ids = extract_coinset_tx_ids_from_offer_payload(&payload);
    Ok(())
}

pub fn ws_error(err: &tokio_tungstenite::tungstenite::Error) -> SignerError {
    SignerError::Other(format!("coinset_ws_once_error:{err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::watchlist::cache::CoinWatchlistCache;
    use tempfile::tempdir;

    fn open_store() -> (tempfile::TempDir, SqliteStore) {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("state.db");
        let store = SqliteStore::open(&path).expect("open");
        (dir, store)
    }

    #[test]
    fn handle_ws_text_routes_mempool_and_confirmed_tx_ids() {
        let (_dir, store) = open_store();
        let watchlist = CoinWatchlistCache::new();
        let tx_id = "c".repeat(64);
        handle_ws_text(
            &store,
            &watchlist,
            &json!({"event": "mempool_seen", "tx_id": tx_id}).to_string(),
        )
        .expect("mempool");
        handle_ws_text(
            &store,
            &watchlist,
            &json!({"event": "tx_confirmed", "tx_id": tx_id}).to_string(),
        )
        .expect("confirmed");

        let events = store
            .list_recent_audit_events(
                Some(&[COINSET_WS_MEMPOOL_EVENT, COINSET_WS_TX_BLOCK_EVENT]),
                None,
                10,
            )
            .expect("events");
        let types: Vec<_> = events.iter().map(|row| row.event_type.as_str()).collect();
        assert_eq!(types.len(), 2);
        assert!(types.contains(&COINSET_WS_MEMPOOL_EVENT));
        assert!(types.contains(&COINSET_WS_TX_BLOCK_EVENT));
    }

    #[test]
    fn handle_ws_text_emits_coin_observed_audit() {
        let (_dir, store) = open_store();
        let watchlist = CoinWatchlistCache::new();
        let coin_id = "d".repeat(64);
        handle_ws_text(
            &store,
            &watchlist,
            &json!({"involved_coins": [format!("0x{coin_id}")]}).to_string(),
        )
        .expect("coin");
        let events = store
            .list_recent_audit_events(Some(&[COINSET_WS_COIN_OBSERVED]), None, 5)
            .expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, COINSET_WS_COIN_OBSERVED);
    }

    #[test]
    fn handle_ws_text_emits_parse_error_for_invalid_json() {
        let (_dir, store) = open_store();
        let watchlist = CoinWatchlistCache::new();
        handle_ws_text(&store, &watchlist, "{not-json").expect("parse error audit");
        let events = store
            .list_recent_audit_events(Some(&[COINSET_WS_PAYLOAD_PARSE_ERROR]), None, 5)
            .expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, COINSET_WS_PAYLOAD_PARSE_ERROR);
    }
}
