use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::config::ManagerProgramConfig;
use crate::daemon::watchlist::cache::CoinWatchlistCache;
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::handler::{handle_ws_text, run_recovery_poll, ws_error};
use super::url::{ensure_rustls_crypto_provider, resolve_coinset_ws_url};

pub async fn capture_coinset_websocket_once(
    store: &SqliteStore,
    program: &ManagerProgramConfig,
    coinset_base_url: &str,
    coin_watchlist: &CoinWatchlistCache,
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
                            handle_ws_text(store, coin_watchlist, &text)?;
                        }
                        Ok(Some(Ok(Message::Ping(payload)))) => {
                            ws.send(Message::Pong(payload)).await.map_err(ws_error)?;
                        }
                        Ok(Some(Ok(Message::Close(_)))) => {
                            return Err(crate::error::SignerError::Other(
                                "coinset_ws_once_closed".to_string(),
                            ));
                        }
                        Ok(Some(Err(err))) => {
                            return Err(ws_error(err));
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
