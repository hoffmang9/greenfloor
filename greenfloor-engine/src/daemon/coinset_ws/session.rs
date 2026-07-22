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

use super::dispatch::{handle_ws_text, run_recovery_poll, ws_error};
use super::url::{merge_ws_p2_filters, resolve_coinset_ws_url_with_p2s, ws_p2_filters_expanded};

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
    /// Drop connection and reconnect immediately (filter expansion or explicit request).
    Reconnect,
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

fn desired_ws_filter_p2s(params: &WsSessionParams<'_>) -> Vec<String> {
    let maker_p2s = params.store.list_watched_p2s().unwrap_or_default();
    merge_ws_p2_filters(params.ctx.p2_index().p2s(), &maker_p2s)
}

/// True when durable maker watches (or inventory reload) need a wider `p2` filter set.
fn filters_need_reconnect(params: &WsSessionParams<'_>, connected: &[String]) -> bool {
    params.ctx.take_reconnect_requested()
        || ws_p2_filters_expanded(connected, &desired_ws_filter_p2s(params))
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
    let filter_p2s = desired_ws_filter_p2s(params);
    let ws_url =
        resolve_coinset_ws_url_with_p2s(params.program, params.coinset_base_url, &filter_p2s);
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
                if filters_need_reconnect(params, &filter_p2s) {
                    return Ok(WsReadEnd::Reconnect);
                }
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

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::Duration;

    use futures_util::StreamExt;
    use mockito::Server;
    use tempfile::tempdir;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    use super::*;
    use crate::operator_log::{
        COINSET_WS_CONNECTED, COINSET_WS_CONNECTING, COINSET_WS_DISCONNECTED,
    };

    fn sample_params<'a>(
        store: &'a SqliteStore,
        program: &'a ManagerProgramConfig,
        coinset_base_url: &'a str,
        ctx: &'a CoinsetWsShared,
    ) -> WsSessionParams<'a> {
        WsSessionParams {
            store,
            program,
            coinset_base_url,
            ctx,
            recovery_reason: "test",
            audits: WsSessionAudits {
                connecting: Some(COINSET_WS_CONNECTING),
                connected: COINSET_WS_CONNECTED,
                disconnected: COINSET_WS_DISCONNECTED,
                text_error: None,
            },
            on_text_error: OnTextError::LogContinue,
            strict_audit: false,
        }
    }

    #[test]
    fn filters_need_reconnect_when_durable_maker_p2_expands_connected_set() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let ctx = CoinsetWsShared::empty();
        let program = ManagerProgramConfig::default();
        let params = sample_params(&store, &program, "", &ctx);
        let inventory_p2 = "aa".repeat(32);
        let maker_p2 = "bb".repeat(32);
        let connected = vec![inventory_p2];

        assert!(
            !filters_need_reconnect(&params, &connected),
            "empty watches must not force reconnect"
        );

        store
            .replace_offer_coin_watches(
                &"cd".repeat(32),
                "m1",
                &[],
                std::slice::from_ref(&maker_p2),
            )
            .expect("watch");
        assert!(
            filters_need_reconnect(&params, &connected),
            "new maker p2 outside connected filters must reconnect"
        );

        let connected_with_maker = desired_ws_filter_p2s(&params);
        assert!(
            !filters_need_reconnect(&params, &connected_with_maker),
            "already-subscribed maker p2 must not reconnect"
        );
    }

    #[test]
    fn filters_need_reconnect_honors_explicit_reconnect_request() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let ctx = CoinsetWsShared::empty();
        let program = ManagerProgramConfig::default();
        let params = sample_params(&store, &program, "", &ctx);
        let connected = desired_ws_filter_p2s(&params);
        assert!(!filters_need_reconnect(&params, &connected));
        ctx.request_reconnect();
        assert!(filters_need_reconnect(&params, &connected));
        assert!(
            !filters_need_reconnect(&params, &connected),
            "take_reconnect_requested must clear the flag"
        );
    }

    #[tokio::test]
    async fn run_ws_session_returns_reconnect_when_maker_p2_watch_lands() {
        use std::cell::Cell;

        use super::super::url::ensure_rustls_crypto_provider;

        ensure_rustls_crypto_provider();
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let ctx = CoinsetWsShared::empty();

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let hold = Arc::new(AtomicBool::new(true));
        let hold_flag = Arc::clone(&hold);
        tokio::spawn(async move {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let Ok(mut ws) = accept_async(stream).await else {
                return;
            };
            while hold_flag.load(Ordering::SeqCst) {
                match tokio::time::timeout(Duration::from_millis(50), ws.next()).await {
                    Ok(Some(Ok(_))) | Err(_) => {}
                    Ok(Some(Err(_)) | None) => break,
                }
            }
        });

        let mut server = Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_all_mempool_tx_ids")
            .with_status(200)
            .with_body(r#"{"success":true,"tx_ids":[]}"#)
            .create_async()
            .await;

        let program = ManagerProgramConfig {
            tx_block_websocket_url: format!("ws://{addr}/ws"),
            ..Default::default()
        };
        let coinset_base = server.url();
        let params = sample_params(&store, &program, &coinset_base, &ctx);
        let expanded = Cell::new(false);

        // Expand maker watches from next_wait after connect so the following
        // session tick sees filter growth and returns Reconnect (same thread;
        // SqliteStore is not Send).
        let end = tokio::time::timeout(
            Duration::from_secs(5),
            run_ws_session(
                &params,
                || false,
                || {
                    if !expanded.get() {
                        let connected = store
                            .list_recent_audit_events(Some(&[COINSET_WS_CONNECTED]), None, 5)
                            .unwrap_or_default()
                            .iter()
                            .any(|event| event.event_type == COINSET_WS_CONNECTED);
                        if connected {
                            store
                                .replace_offer_coin_watches(
                                    &"ab".repeat(32),
                                    "m1",
                                    &[],
                                    &["cd".repeat(32)],
                                )
                                .expect("expand watches");
                            expanded.set(true);
                        }
                    }
                    Duration::from_millis(50)
                },
            ),
        )
        .await
        .expect("session timeout")
        .expect("session ok");
        hold.store(false, Ordering::SeqCst);
        assert_eq!(end, WsReadEnd::Reconnect);
    }
}
