use std::time::Duration;

use serde_json::json;

use crate::config::ManagerProgramConfig;
use crate::error::SignerResult;
use crate::operator_log::{
    LogContext, COINSET_WS_ONCE_CONNECTED, COINSET_WS_ONCE_DISCONNECTED, COINSET_WS_ONCE_STARTED,
};
use crate::storage::SqliteStore;

use super::once_timings::OnceCaptureTimings;
use super::process_context::CoinsetWsShared;
use super::session::{run_ws_session, OnTextError, WsSessionAudits, WsSessionParams};
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
    let OnceCaptureTimings {
        capture_window,
        reconnect,
    } = timings;
    let deadline = tokio::time::Instant::now() + capture_window;

    while tokio::time::Instant::now() < deadline {
        let params = WsSessionParams {
            store,
            program,
            coinset_base_url,
            ctx,
            recovery_reason: "once_start",
            audits: WsSessionAudits {
                connecting: None,
                connected: COINSET_WS_ONCE_CONNECTED,
                disconnected: COINSET_WS_ONCE_DISCONNECTED,
                text_error: None,
            },
            on_text_error: OnTextError::Propagate,
            strict_audit: true,
        };
        match run_ws_session(
            &params,
            || tokio::time::Instant::now() >= deadline,
            || {
                deadline
                    .saturating_duration_since(tokio::time::Instant::now())
                    .min(Duration::from_secs(1))
            },
        )
        .await
        {
            Ok(_) => {
                LogContext::COINSET.audit(
                    store,
                    COINSET_WS_ONCE_DISCONNECTED,
                    &json!({"ws_url": ws_url, "reason": "capture_window"}),
                    None,
                )?;
                return Ok(());
            }
            Err(err) => {
                if tokio::time::Instant::now() + reconnect >= deadline {
                    return Err(err);
                }
                tokio::time::sleep(reconnect).await;
            }
        }
    }
    Ok(())
}
