use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::config::ManagerProgramConfig;
use crate::daemon::watchlist::cache::CoinWatchlistCache;
use crate::error::SignerResult;
use crate::operator_log::{
    LogContext, COINSET_WS_ONCE_CONNECTED, COINSET_WS_ONCE_DISCONNECTED, COINSET_WS_ONCE_STARTED,
};
use crate::storage::SqliteStore;

use super::handler::{handle_ws_text, run_recovery_poll, ws_error};
use super::once_timings::OnceCaptureTimings;
use super::url::{ensure_rustls_crypto_provider, resolve_coinset_ws_url};

pub async fn capture_coinset_websocket_once(
    store: &SqliteStore,
    program: &ManagerProgramConfig,
    coinset_base_url: &str,
    coin_watchlist: &CoinWatchlistCache,
) -> SignerResult<()> {
    capture_coinset_websocket_once_with_timings(
        store,
        program,
        coinset_base_url,
        coin_watchlist,
        OnceCaptureTimings::from_program(program),
    )
    .await
}

pub async fn capture_coinset_websocket_once_with_timings(
    store: &SqliteStore,
    program: &ManagerProgramConfig,
    coinset_base_url: &str,
    coin_watchlist: &CoinWatchlistCache,
    timings: OnceCaptureTimings,
) -> SignerResult<()> {
    ensure_rustls_crypto_provider();
    let ws_url = resolve_coinset_ws_url(program, coinset_base_url);
    LogContext::COINSET.audit(
        store,
        COINSET_WS_ONCE_STARTED,
        &json!({"ws_url": ws_url}),
        None,
    )?;
    let _ = run_recovery_poll(store, program, coinset_base_url, "once_start").await;
    let OnceCaptureTimings {
        capture_window,
        reconnect,
    } = timings;
    let deadline = tokio::time::Instant::now() + capture_window;

    while tokio::time::Instant::now() < deadline {
        match connect_async(&ws_url).await {
            Ok((mut ws, _response)) => {
                LogContext::COINSET.audit(
                    store,
                    COINSET_WS_ONCE_CONNECTED,
                    &json!({"ws_url": ws_url}),
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
                            ws.send(Message::Pong(payload))
                                .await
                                .map_err(|err| ws_error(&err))?;
                        }
                        Ok(Some(Ok(Message::Close(_)))) => {
                            return Err(crate::error::SignerError::Other(
                                "coinset_ws_once_closed".to_string(),
                            ));
                        }
                        Ok(Some(Err(err))) => {
                            return Err(ws_error(&err));
                        }
                        Ok(None) => break,
                        _ => {}
                    }
                }
            }
            Err(err) => {
                LogContext::COINSET.audit(
                    store,
                    COINSET_WS_ONCE_DISCONNECTED,
                    &json!({"error": err.to_string()}),
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
