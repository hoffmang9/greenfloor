//! Shared Coinset WS connect / read pump for daemon loop and `--once` capture.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::config::ManagerProgramConfig;
use crate::daemon::coinset_ws::CoinsetWsShared;
use crate::error::SignerResult;
use crate::operator_log::LogContext;
use crate::storage::SqliteStore;

use super::dispatch::run_recovery_poll;
use super::handler::{handle_ws_text, ws_error};
use super::url::resolve_coinset_ws_url_with_p2s;

/// How text-handler failures are treated inside the read pump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OnTextError {
    /// Log + best-effort audit, keep reading (daemon loop).
    LogContinue,
    /// Propagate as `SignerError` (CLI `--once`).
    Propagate,
}

/// Why the inner read loop stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WsReadEnd {
    StreamEnded,
    Stopped,
}

/// Audit event names for one connect attempt.
pub(crate) struct WsSessionAudits {
    pub connecting: Option<&'static str>,
    pub connected: &'static str,
    pub disconnected: &'static str,
    pub text_error: Option<&'static str>,
}

pub(crate) struct WsSessionParams<'a> {
    pub store: &'a SqliteStore,
    pub program: &'a ManagerProgramConfig,
    pub coinset_base_url: &'a str,
    pub ctx: &'a CoinsetWsShared,
    pub recovery_reason: &'a str,
    pub audits: WsSessionAudits,
    pub on_text_error: OnTextError,
    /// When true, use `audit` (fallible); when false, `audit_best_effort`.
    pub strict_audit: bool,
}

fn audit(
    params: &WsSessionParams<'_>,
    event: &str,
    payload: &serde_json::Value,
) -> SignerResult<()> {
    if params.strict_audit {
        LogContext::COINSET.audit(params.store, event, payload, None)
    } else {
        LogContext::COINSET.audit_best_effort(params.store, event, payload, None);
        Ok(())
    }
}

/// Resolve URL, run recovery poll, connect, and pump frames until `should_stop` or stream end.
///
/// `next_wait` supplies the per-iteration `timeout` duration (daemon: 1s; once: min(remaining, 1s)).
///
/// # Errors
///
/// Returns an error on connect failure (when `strict_audit` / propagate policy requires it),
/// stream errors under [`OnTextError::Propagate`], or text-handler failures under Propagate.
pub(crate) async fn run_ws_session(
    params: &WsSessionParams<'_>,
    mut should_stop: impl FnMut() -> bool,
    mut next_wait: impl FnMut() -> Duration,
) -> SignerResult<WsReadEnd> {
    let ws_url = resolve_coinset_ws_url_with_p2s(
        params.program,
        params.coinset_base_url,
        params.ctx.p2_index().p2s(),
    );
    if let Some(connecting) = params.audits.connecting {
        audit(params, connecting, &json!({"ws_url": ws_url}))?;
    }
    let _ = run_recovery_poll(
        params.store,
        params.program,
        params.coinset_base_url,
        params.recovery_reason,
    )
    .await;
    match connect_async(&ws_url).await {
        Ok((mut ws, _response)) => {
            audit(params, params.audits.connected, &json!({"ws_url": ws_url}))?;
            while !should_stop() {
                let wait_for = next_wait();
                match tokio::time::timeout(wait_for, ws.next()).await {
                    Ok(Some(Ok(Message::Text(text)))) => {
                        if let Err(err) = handle_ws_text(params.store, params.ctx, &text) {
                            match params.on_text_error {
                                OnTextError::LogContinue => {
                                    tracing::warn!(
                                        error = %err,
                                        "coinset websocket payload handler failed"
                                    );
                                    if let Some(event) = params.audits.text_error {
                                        let _ = audit(
                                            params,
                                            event,
                                            &json!({"error": err.to_string()}),
                                        );
                                    }
                                }
                                OnTextError::Propagate => return Err(err),
                            }
                        }
                    }
                    Ok(Some(Ok(Message::Ping(payload)))) => {
                        let _ = ws.send(Message::Pong(payload)).await;
                    }
                    Ok(Some(Ok(Message::Close(_))) | None) => {
                        return Ok(WsReadEnd::StreamEnded);
                    }
                    Ok(Some(Err(err))) => {
                        return match params.on_text_error {
                            OnTextError::Propagate => Err(ws_error(&err)),
                            OnTextError::LogContinue => Ok(WsReadEnd::StreamEnded),
                        };
                    }
                    Ok(Some(Ok(_))) | Err(_) => {}
                }
            }
            Ok(WsReadEnd::Stopped)
        }
        Err(err) => {
            audit(
                params,
                params.audits.disconnected,
                &json!({"ws_url": ws_url, "error": err.to_string()}),
            )?;
            Err(ws_error(&err))
        }
    }
}
