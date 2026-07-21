use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::config::ManagerProgramConfig;
use crate::operator_log::{
    COINSET_WS_CONNECTED, COINSET_WS_CONNECTING, COINSET_WS_DISCONNECTED, COINSET_WS_ONCE_ERROR,
};
use crate::storage::SqliteStore;

use super::process_context::CoinsetWsShared;
use super::session::{run_ws_session, OnTextError, WsSessionAudits, WsSessionParams};
use super::url::ensure_rustls_crypto_provider;

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
#[must_use]
pub fn start_coinset_websocket_loop(
    db_path: PathBuf,
    program: ManagerProgramConfig,
    coinset_base_url: String,
    coinset: Arc<CoinsetWsShared>,
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
            coinset,
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
    coinset: Arc<CoinsetWsShared>,
    stop: Arc<AtomicBool>,
) {
    let Ok(store) = SqliteStore::open(&db_path) else {
        return;
    };
    let reconnect =
        Duration::from_secs(program.tx_block_websocket_reconnect_interval_seconds.max(1));

    while !stop.load(Ordering::SeqCst) {
        let params = WsSessionParams {
            store: &store,
            program: &program,
            coinset_base_url: &coinset_base_url,
            ctx: &coinset,
            recovery_reason: "connected",
            audits: WsSessionAudits {
                connecting: Some(COINSET_WS_CONNECTING),
                connected: COINSET_WS_CONNECTED,
                disconnected: COINSET_WS_DISCONNECTED,
                text_error: Some(COINSET_WS_ONCE_ERROR),
            },
            on_text_error: OnTextError::LogContinue,
            strict_audit: false,
        };
        let _ = run_ws_session(
            &params,
            || stop.load(Ordering::SeqCst) || coinset.take_reconnect_requested(),
            || Duration::from_secs(1),
        )
        .await;
        if stop.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(reconnect).await;
    }
}

#[cfg(test)]
mod tests {
    use super::super::capture::capture_coinset_websocket_once_with_timings;
    use super::super::once_timings::OnceCaptureTimings;
    use super::super::url::resolve_coinset_ws_url_with_p2s;
    use super::*;
    use mockito::Server;
    use tempfile::tempdir;

    fn sample_program() -> ManagerProgramConfig {
        ManagerProgramConfig {
            runtime_market_slot_count: 1,
            runtime_offer_parallelism_max_workers: 2,
            tx_block_websocket_url: "ws://127.0.0.1:9/ws".to_string(),
            tx_block_websocket_reconnect_interval_seconds: 0,
            tx_block_fallback_poll_interval_seconds: 0,
            ..Default::default()
        }
    }

    #[test]
    fn resolve_coinset_ws_url_appends_required_filters() {
        let program = ManagerProgramConfig::default();
        let url = resolve_coinset_ws_url_with_p2s(&program, "https://api.coinset.org", &[]);
        assert!(url.contains("events=transaction,offer"));
        assert!(url.contains("tx_status=pending,confirmed"));
    }

    #[tokio::test]
    async fn capture_once_runs_recovery_poll_and_records_started() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_all_mempool_tx_ids")
            .with_status(200)
            .with_body(r#"{"success":true,"tx_ids":[]}"#)
            .create_async()
            .await;
        let program = ManagerProgramConfig {
            tx_block_websocket_url: "ws://127.0.0.1:9/ws".to_string(),
            ..Default::default()
        };
        let _ = capture_coinset_websocket_once_with_timings(
            &store,
            &program,
            &server.url(),
            &CoinsetWsShared::empty(),
            OnceCaptureTimings {
                capture_window: Duration::from_millis(50),
                reconnect: Duration::from_millis(10),
            },
        )
        .await;

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

    #[test]
    fn loop_handle_stop_joins_background_thread() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let program = sample_program();
        let mut handle = start_coinset_websocket_loop(
            db_path,
            program,
            "https://example.test".to_string(),
            CoinsetWsShared::empty(),
        );
        handle.stop();
    }

    #[tokio::test]
    async fn run_loop_exits_immediately_when_stop_requested() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let stop = Arc::new(AtomicBool::new(true));
        run_coinset_websocket_loop(
            db_path,
            sample_program(),
            "https://example.test".to_string(),
            CoinsetWsShared::empty(),
            stop,
        )
        .await;
    }

    #[tokio::test]
    async fn run_loop_records_disconnect_audit_on_bad_endpoint() {
        use std::time::Instant;

        use crate::operator_log::{COINSET_WS_CONNECTING, COINSET_WS_DISCONNECTED};

        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();
        let program = ManagerProgramConfig {
            runtime_market_slot_count: 1,
            runtime_offer_parallelism_max_workers: 2,
            tx_block_websocket_url: "ws://127.0.0.1:1/ws".to_string(),
            tx_block_websocket_reconnect_interval_seconds: 0,
            tx_block_fallback_poll_interval_seconds: 0,
            ..Default::default()
        };

        let mut server = Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_all_mempool_tx_ids")
            .with_status(200)
            .with_body(r#"{"success":true,"tx_ids":[]}"#)
            .create_async()
            .await;
        let coinset_base_url = server.url();

        let handle = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            runtime.block_on(run_coinset_websocket_loop(
                db_path,
                program,
                coinset_base_url,
                CoinsetWsShared::empty(),
                stop_flag,
            ));
        });

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let events = store
                .list_recent_audit_events(
                    Some(&[COINSET_WS_CONNECTING, COINSET_WS_DISCONNECTED]),
                    None,
                    10,
                )
                .expect("events");
            let event_types: std::collections::HashSet<&str> = events
                .iter()
                .map(|event| event.event_type.as_str())
                .collect();
            if event_types.contains(COINSET_WS_CONNECTING)
                && event_types.contains(COINSET_WS_DISCONNECTED)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for websocket connect/disconnect audit events"
            );
            std::thread::sleep(Duration::from_millis(50));
        }

        stop.store(true, Ordering::SeqCst);
        handle.join().expect("join");
    }
}
