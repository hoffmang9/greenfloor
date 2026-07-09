use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::config::ManagerProgramConfig;
use crate::error::SignerResult;
use crate::operator_log::{
    LogContext, COINSET_WS_ONCE_CONNECTED, COINSET_WS_ONCE_DISCONNECTED, COINSET_WS_ONCE_STARTED,
};
use crate::storage::SqliteStore;

use super::handler::{handle_ws_text, run_recovery_poll, ws_error};
use super::once_timings::OnceCaptureTimings;
use super::process_context::CoinsetWsShared;
use super::url::{ensure_rustls_crypto_provider, resolve_coinset_ws_url_with_p2s};

pub async fn capture_coinset_websocket_once(
    store: &SqliteStore,
    program: &ManagerProgramConfig,
    coinset_base_url: &str,
    ctx: &CoinsetWsShared,
) -> SignerResult<()> {
    capture_coinset_websocket_once_with_timings(
        store,
        program,
        coinset_base_url,
        ctx,
        OnceCaptureTimings::from_program(program),
    )
    .await
}

pub async fn capture_coinset_websocket_once_with_timings(
    store: &SqliteStore,
    program: &ManagerProgramConfig,
    coinset_base_url: &str,
    ctx: &CoinsetWsShared,
    timings: OnceCaptureTimings,
) -> SignerResult<()> {
    ensure_rustls_crypto_provider();
    let ws_url = resolve_coinset_ws_url_with_p2s(program, coinset_base_url, ctx.p2_index().p2s());
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
                            handle_ws_text(store, ctx, &text)?;
                        }
                        Ok(Some(Ok(Message::Ping(payload)))) => {
                            let _ = ws.send(Message::Pong(payload)).await;
                        }
                        Ok(Some(Err(err))) => return Err(ws_error(&err)),
                        Ok(None | Some(Ok(Message::Close(_)))) => break,
                        _ => {}
                    }
                }
                LogContext::COINSET.audit(
                    store,
                    COINSET_WS_ONCE_DISCONNECTED,
                    &json!({"ws_url": ws_url, "reason": "capture_window"}),
                    None,
                )?;
                return Ok(());
            }
            Err(err) => {
                LogContext::COINSET.audit(
                    store,
                    COINSET_WS_ONCE_DISCONNECTED,
                    &json!({"ws_url": ws_url, "error": err.to_string()}),
                    None,
                )?;
                if tokio::time::Instant::now() + reconnect >= deadline {
                    return Err(ws_error(&err));
                }
                tokio::time::sleep(reconnect).await;
            }
        }
    }
    Ok(())
}
