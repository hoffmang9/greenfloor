use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::daemon::watchlist::cache::CoinWatchlistCache;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::config::ManagerProgramConfig;
use crate::operator_log::{
    audit_coinset, AuditDurability, COINSET_WS_CONNECTED, COINSET_WS_CONNECTING,
    COINSET_WS_DISCONNECTED,
};
use crate::storage::SqliteStore;

use super::handler::{handle_ws_text, run_recovery_poll};
use super::url::{ensure_rustls_crypto_provider, resolve_coinset_ws_url};

pub struct CoinsetWebsocketLoopHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl CoinsetWebsocketLoopHandle {
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for CoinsetWebsocketLoopHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Start the Coinset websocket background loop.
///
/// # Panics
///
/// Panics if the dedicated Tokio runtime cannot be constructed.
pub fn start_coinset_websocket_loop(
    db_path: PathBuf,
    program: ManagerProgramConfig,
    coinset_base_url: String,
    coin_watchlist: Arc<CoinWatchlistCache>,
) -> CoinsetWebsocketLoopHandle {
    ensure_rustls_crypto_provider();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = stop.clone();
    let join = thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("coinset websocket loop runtime");
        runtime.block_on(run_coinset_websocket_loop(
            db_path,
            program,
            coinset_base_url,
            coin_watchlist,
            stop_flag,
        ));
    });
    CoinsetWebsocketLoopHandle {
        stop,
        join: Some(join),
    }
}

async fn run_coinset_websocket_loop(
    db_path: PathBuf,
    program: ManagerProgramConfig,
    coinset_base_url: String,
    coin_watchlist: Arc<CoinWatchlistCache>,
    stop: Arc<AtomicBool>,
) {
    let Ok(store) = SqliteStore::open(&db_path) else {
        return;
    };
    let ws_url = resolve_coinset_ws_url(&program, &coinset_base_url);
    let reconnect =
        Duration::from_secs(program.tx_block_websocket_reconnect_interval_seconds.max(1));

    while !stop.load(Ordering::SeqCst) {
        let _ = run_recovery_poll(&store, &program, &coinset_base_url, "connected").await;
        let _ = audit_coinset(
            &store,
            COINSET_WS_CONNECTING,
            &json!({"ws_url": ws_url}),
            None,
            AuditDurability::BestEffort,
        );
        match connect_async(&ws_url).await {
            Ok((mut ws, _response)) => {
                let _ = audit_coinset(
                    &store,
                    COINSET_WS_CONNECTED,
                    &json!({"ws_url": ws_url}),
                    None,
                    AuditDurability::BestEffort,
                );
                while !stop.load(Ordering::SeqCst) {
                    match tokio::time::timeout(Duration::from_secs(1), ws.next()).await {
                        Ok(Some(Ok(Message::Text(text)))) => {
                            let _ = handle_ws_text(&store, &coin_watchlist, &text);
                        }
                        Ok(Some(Ok(Message::Ping(payload)))) => {
                            let _ = ws.send(Message::Pong(payload)).await;
                        }
                        Ok(None | Some(Ok(Message::Close(_)) | Err(_))) => break,
                        _ => {}
                    }
                }
            }
            Err(err) => {
                let _ = audit_coinset(
                    &store,
                    COINSET_WS_DISCONNECTED,
                    &json!({"error": err.to_string()}),
                    None,
                    AuditDurability::BestEffort,
                );
            }
        }
        if stop.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(reconnect).await;
    }
}

#[cfg(test)]
mod tests {
    use super::super::capture::capture_coinset_websocket_once;
    use super::super::url::resolve_coinset_ws_url;
    use super::*;
    use mockito::Server;
    use tempfile::tempdir;

    fn sample_program() -> ManagerProgramConfig {
        ManagerProgramConfig {
            runtime_market_slot_count: 1,
            runtime_offer_parallelism_max_workers: 2,
            tx_block_websocket_url: "ws://127.0.0.1:9/ws".to_string(),
            tx_block_websocket_reconnect_interval_seconds: 1,
            tx_block_fallback_poll_interval_seconds: 1,
            ..Default::default()
        }
    }

    #[test]
    fn resolve_coinset_ws_url_prefers_program_override() {
        let program = sample_program();
        assert_eq!(
            resolve_coinset_ws_url(&program, "https://api.coinset.org"),
            "ws://127.0.0.1:9/ws"
        );
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
        capture_coinset_websocket_once(&store, &program, &server.url(), &CoinWatchlistCache::new())
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
