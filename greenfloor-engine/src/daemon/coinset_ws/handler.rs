use serde_json::Value;

use crate::coinset::get_all_mempool_tx_ids;
use crate::config::ManagerProgramConfig;
use crate::daemon::coinset_tx::{
    classify_ws_payload_tx_ids, extract_coin_ids_from_offer_payload,
    extract_coinset_tx_ids_from_offer_payload,
};
use crate::daemon::watchlist::match_watched_coin_ids;
use crate::error::{SignerError, SignerResult};
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
            store.add_audit_event(
                "coinset_ws_recovery_poll",
                &serde_json::json!({"reason": reason, "tx_id_count": tx_ids.len()}),
                None,
            )?;
            if new_count > 0 {
                store.add_audit_event(
                    "mempool_observed",
                    &serde_json::json!({"new_tx_ids": new_count, "source": "coinset_websocket"}),
                    None,
                )?;
            }
            Ok(())
        }
        Err(err) => {
            store.add_audit_event(
                "coinset_ws_recovery_poll_error",
                &serde_json::json!({"reason": reason, "error": err.to_string()}),
                None,
            )?;
            Err(err)
        }
    }
}

pub fn handle_ws_text(store: &SqliteStore, raw: &str) -> SignerResult<()> {
    let payload: Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(_) => {
            store.add_audit_event(
                "coinset_ws_payload_parse_error",
                &serde_json::json!({"raw": raw.chars().take(200).collect::<String>()}),
                None,
            )?;
            return Ok(());
        }
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
        store.add_audit_event(
            "coinset_ws_payload_ignored",
            &serde_json::json!({"kind": kind}),
            None,
        )?;
        return Ok(());
    }
    let (mempool_tx_ids, confirmed_tx_ids) = classify_ws_payload_tx_ids(&payload);
    if !mempool_tx_ids.is_empty() {
        let new_count = store.observe_mempool_tx_ids(&mempool_tx_ids)?;
        store.add_audit_event(
            "coinset_ws_mempool_event",
            &serde_json::json!({"tx_id_count": mempool_tx_ids.len()}),
            None,
        )?;
        if new_count > 0 {
            store.add_audit_event(
                "mempool_observed",
                &serde_json::json!({"new_tx_ids": new_count, "source": "coinset_websocket"}),
                None,
            )?;
        }
    }
    if !confirmed_tx_ids.is_empty() {
        let confirmed = store.confirm_tx_ids(&confirmed_tx_ids)?;
        store.add_audit_event(
            "coinset_ws_tx_block_event",
            &serde_json::json!({"tx_id_count": confirmed_tx_ids.len(), "confirmed_count": confirmed}),
            None,
        )?;
        store.add_audit_event(
            "tx_block_confirmed",
            &serde_json::json!({
                "tx_ids": confirmed_tx_ids,
                "confirmed_count": confirmed,
                "source": "coinset_websocket",
            }),
            None,
        )?;
    }
    let observed_coin_ids = extract_coin_ids_from_offer_payload(&payload);
    if !observed_coin_ids.is_empty() {
        store.add_audit_event(
            "coinset_ws_coin_observed",
            &serde_json::json!({"coin_id_count": observed_coin_ids.len()}),
            None,
        )?;
        let hits = match_watched_coin_ids(&observed_coin_ids);
        if !hits.is_empty() {
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
                        Value::Array(
                            coin_ids
                                .into_iter()
                                .take(10)
                                .map(Value::String)
                                .collect(),
                        ),
                    )
                })
                .collect();
            store.add_audit_event(
                "coin_watch_hit",
                &serde_json::json!({
                    "coin_id_count": observed_coin_ids.len(),
                    "coin_ids_sample": sample,
                    "market_hits": market_hits,
                    "source": "coinset_websocket",
                }),
                None,
            )?;
        }
    }
    let _coinset_tx_ids = extract_coinset_tx_ids_from_offer_payload(&payload);
    Ok(())
}

pub fn ws_error(err: tokio_tungstenite::tungstenite::Error) -> SignerError {
    SignerError::Other(format!("coinset_ws_once_error:{err}"))
}
