use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::coinset::get_all_mempool_tx_ids;
use crate::config::ManagerProgramConfig;
use crate::daemon::coinset_tx::{
    classify_ws_payload_tx_ids, extract_coinset_tx_ids_from_offer_payload,
};
use crate::error::{SignerError, SignerResult};
use crate::storage::SqliteStore;

fn ensure_rustls_crypto_provider() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

pub fn resolve_coinset_ws_url(program: &ManagerProgramConfig, coinset_base_url: &str) -> String {
    let configured = program.tx_block_websocket_url.trim();
    if !configured.is_empty() {
        return configured.to_string();
    }
    let base_url = coinset_base_url.trim();
    if base_url.is_empty() {
        return if program.network.eq_ignore_ascii_case("testnet11")
            || program.network.eq_ignore_ascii_case("testnet")
        {
            "wss://testnet11.api.coinset.org/ws".to_string()
        } else {
            "wss://api.coinset.org/ws".to_string()
        };
    }
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.starts_with("https://") {
        return format!("wss://{}", trimmed.trim_start_matches("https://"));
    }
    if trimmed.starts_with("http://") {
        return format!("ws://{}", trimmed.trim_start_matches("http://"));
    }
    trimmed.to_string()
}

pub async fn capture_coinset_websocket_once(
    store: &SqliteStore,
    program: &ManagerProgramConfig,
    coinset_base_url: &str,
) -> SignerResult<()> {
    ensure_rustls_crypto_provider();
    let ws_url = resolve_coinset_ws_url(program, coinset_base_url);
    store.add_audit_event(
        "coinset_ws_once_started",
        &serde_json::json!({"ws_url": ws_url}),
        None,
    )?;
    let _ = run_recovery_poll(store, program, coinset_base_url, "once_start").await;
    let capture_window =
        Duration::from_secs(program.tx_block_fallback_poll_interval_seconds.max(1));
    let reconnect =
        Duration::from_secs(program.tx_block_websocket_reconnect_interval_seconds.max(1));
    let deadline = tokio::time::Instant::now() + capture_window;

    while tokio::time::Instant::now() < deadline {
        match connect_async(&ws_url).await {
            Ok((mut ws, _response)) => {
                store.add_audit_event(
                    "coinset_ws_once_connected",
                    &serde_json::json!({"ws_url": ws_url}),
                    None,
                )?;
                while tokio::time::Instant::now() < deadline {
                    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                    let wait_for = remaining.min(Duration::from_secs(1));
                    match tokio::time::timeout(wait_for, ws.next()).await {
                        Ok(Some(Ok(Message::Text(text)))) => {
                            handle_ws_text(store, &text)?;
                        }
                        Ok(Some(Ok(Message::Ping(payload)))) => {
                            ws.send(Message::Pong(payload)).await.map_err(ws_error)?;
                        }
                        Ok(Some(Ok(Message::Close(_)))) => {
                            return Err(SignerError::Other("coinset_ws_once_closed".to_string()));
                        }
                        Ok(Some(Err(err))) => {
                            return Err(SignerError::Other(format!("coinset_ws_once_error:{err}")));
                        }
                        Ok(None) => break,
                        Err(_) => continue,
                        _ => {}
                    }
                }
            }
            Err(err) => {
                store.add_audit_event(
                    "coinset_ws_once_disconnected",
                    &serde_json::json!({"error": err.to_string()}),
                    None,
                )?;
            }
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(reconnect).await;
    }
    Ok(())
}

async fn run_recovery_poll(
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

fn handle_ws_text(store: &SqliteStore, raw: &str) -> SignerResult<()> {
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
    }
    let coinset_tx_ids = extract_coinset_tx_ids_from_offer_payload(&payload);
    if !coinset_tx_ids.is_empty() {
        store.add_audit_event(
            "coinset_ws_coin_observed",
            &serde_json::json!({"coin_id_count": coinset_tx_ids.len()}),
            None,
        )?;
    }
    Ok(())
}

fn ws_error(err: tokio_tungstenite::tungstenite::Error) -> SignerError {
    SignerError::Other(format!("coinset_ws_once_error:{err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn sample_program() -> ManagerProgramConfig {
        ManagerProgramConfig {
            network: "mainnet".to_string(),
            home_dir: PathBuf::from("/tmp/gf"),
            app_log_level: "INFO".to_string(),
            app_log_level_was_missing: false,
            dexie_api_base: "https://api.dexie.space".to_string(),
            splash_api_base: "http://localhost:4000".to_string(),
            offer_publish_venue: "dexie".to_string(),
            coin_ops_minimum_fee_mojos: 0,
            coin_ops_max_operations_per_run: 0,
            coin_ops_max_daily_fee_budget_mojos: 0,
            coin_ops_split_fee_mojos: 0,
            coin_ops_combine_fee_mojos: 0,
            runtime_offer_bootstrap_wait_timeout_seconds: 120,
            runtime_market_slot_count: 1,
            runtime_parallel_markets: false,
            runtime_offer_parallelism_enabled: false,
            runtime_offer_parallelism_max_workers: 2,
            runtime_dry_run: false,
            runtime_loop_interval_seconds: 30,
            tx_block_trigger_mode: "websocket".to_string(),
            tx_block_websocket_url: "ws://127.0.0.1:9/ws".to_string(),
            tx_block_websocket_reconnect_interval_seconds: 1,
            tx_block_fallback_poll_interval_seconds: 1,
        }
    }

    #[tokio::test]
    async fn capture_once_runs_recovery_poll_and_records_started() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");

        let mut server = Server::new_async().await;
        let tx_id = "a".repeat(64);
        let _mock = server
            .mock("POST", "/get_all_mempool_tx_ids")
            .with_status(200)
            .with_body(format!(r#"{{"success":true,"tx_ids":["{tx_id}"]}}"#))
            .create();

        let program = sample_program();
        capture_coinset_websocket_once(&store, &program, &server.url())
            .await
            .expect("capture");

        let events = store
            .list_recent_audit_events(
                Some(&["coinset_ws_once_started", "coinset_ws_recovery_poll"]),
                None,
                10,
            )
            .expect("events");
        let event_types: std::collections::HashSet<String> = events
            .iter()
            .map(|event| event.event_type.clone())
            .collect();
        assert!(event_types.contains("coinset_ws_once_started"));
        assert!(event_types.contains("coinset_ws_recovery_poll"));
    }
}
