use serde_json::Value;
use tracing::Level;

use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::redact::redact_json_for_log;
pub use super::trace_mirror::trace_audit_mirror;

/// Correlation and identity fields shared by operator tracing events.
#[derive(Debug, Clone, Copy)]
pub struct LogContext {
    pub service: &'static str,
    pub phase: &'static str,
}

impl LogContext {
    pub const DAEMON_CYCLE: Self = Self {
        service: "daemon",
        phase: "daemon_cycle",
    };
    pub const MARKET_CYCLE: Self = Self {
        service: "daemon",
        phase: "market_cycle",
    };
    pub const OFFER_POST: Self = Self {
        service: "manager",
        phase: "offer_post",
    };
    pub const CONFIG: Self = Self {
        service: "daemon",
        phase: "config",
    };
    pub const VALIDATION: Self = Self {
        service: "manager",
        phase: "validation",
    };
}

/// Emit one redacted payload blob trace line (tier-2 blob mirror only).
pub fn trace_audit_outcome(
    level: Level,
    ctx: LogContext,
    audit_event_type: &str,
    payload: &Value,
    market_id: Option<&str>,
    trace_message: &'static str,
) {
    let payload_text = redact_json_for_log(payload).to_string();
    crate::event_at_level!(
        level,
        service = ctx.service,
        event = audit_event_type,
        phase = ctx.phase,
        market_id = market_id.unwrap_or(""),
        payload = %payload_text,
        trace_message
    );
}

/// Dispatch `tracing::event!` when the level is chosen at runtime.
#[macro_export]
macro_rules! event_at_level {
    ($level:expr, $($fields:tt)* ) => {
        match $level {
            ::tracing::Level::ERROR => ::tracing::event!(::tracing::Level::ERROR, $($fields)*),
            ::tracing::Level::WARN => ::tracing::event!(::tracing::Level::WARN, $($fields)*),
            ::tracing::Level::INFO => ::tracing::event!(::tracing::Level::INFO, $($fields)*),
            ::tracing::Level::DEBUG | ::tracing::Level::TRACE => {
                ::tracing::event!(::tracing::Level::DEBUG, $($fields)*)
            }
        }
    };
}

/// Persist an audit row and emit one redacted trace line for the same outcome.
///
/// # Errors
///
/// Returns an error when the audit insert fails.
pub fn audit_and_trace(
    store: &SqliteStore,
    level: Level,
    ctx: LogContext,
    audit_event_type: &str,
    payload: &Value,
    market_id: Option<&str>,
    trace_message: &'static str,
) -> SignerResult<()> {
    store.add_audit_event(audit_event_type, payload, market_id)?;
    trace_audit_mirror(
        level,
        ctx,
        audit_event_type,
        payload,
        market_id,
        trace_message,
    );
    Ok(())
}

/// Persist an audit row without emitting a trace line.
///
/// # Errors
///
/// Returns an error when the audit insert fails.
pub fn audit_only(
    store: &SqliteStore,
    audit_event_type: &str,
    payload: &Value,
    market_id: Option<&str>,
) -> SignerResult<()> {
    store.add_audit_event(audit_event_type, payload, market_id)
}

/// Persist a daemon-cycle audit row without a trace mirror (full payloads for `status_cli`).
///
/// # Errors
///
/// Returns an error when the audit insert fails.
pub fn audit_daemon_cycle_only(
    store: &SqliteStore,
    audit_event_type: &str,
    payload: &Value,
) -> SignerResult<()> {
    audit_only(store, audit_event_type, payload, None)
}

/// Persist and trace a config reload audit event (no `market_id`).
///
/// # Errors
///
/// Returns an error when the audit insert fails.
pub fn audit_config(
    store: &SqliteStore,
    level: Level,
    audit_event_type: &str,
    payload: &Value,
    trace_message: &'static str,
) -> SignerResult<()> {
    audit_and_trace(
        store,
        level,
        LogContext::CONFIG,
        audit_event_type,
        payload,
        None,
        trace_message,
    )
}

/// Persist and trace a daemon-cycle audit event (no `market_id`).
///
/// # Errors
///
/// Returns an error when the audit insert fails.
pub fn audit_daemon_cycle(
    store: &SqliteStore,
    level: Level,
    audit_event_type: &str,
    payload: &Value,
    trace_message: &'static str,
) -> SignerResult<()> {
    audit_and_trace(
        store,
        level,
        LogContext::DAEMON_CYCLE,
        audit_event_type,
        payload,
        None,
        trace_message,
    )
}

/// Persist and trace a market-cycle audit event.
///
/// # Errors
///
/// Returns an error when the audit insert fails.
pub fn audit_market_cycle(
    store: &SqliteStore,
    level: Level,
    audit_event_type: &str,
    payload: &Value,
    market_id: &str,
    trace_message: &'static str,
) -> SignerResult<()> {
    audit_and_trace(
        store,
        level,
        LogContext::MARKET_CYCLE,
        audit_event_type,
        payload,
        Some(market_id),
        trace_message,
    )
}

/// Emit a structured operator trace event (`service`, `event`, and `phase` are always set).
///
/// Additional fields use tracing syntax, including `?` for debug formatting.
#[macro_export]
macro_rules! trace_event {
    ($level:ident, $ctx:expr, $event:expr, { $($fields:tt)* } ; $msg:literal) => {
        tracing::event!(
            tracing::Level::$level,
            service = ($ctx).service,
            event = $event,
            phase = ($ctx).phase,
            $($fields)*
            $msg
        );
    };
}

#[cfg(test)]
pub mod trace_capture {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    pub struct TraceCapture {
        buf: Arc<Mutex<Vec<u8>>>,
        _guard: tracing::subscriber::DefaultGuard,
    }

    struct Writer(Arc<Mutex<Vec<u8>>>);

    impl Write for Writer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("lock").extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl TraceCapture {
        pub fn install() -> Self {
            let buf = Arc::new(Mutex::new(Vec::new()));
            let writer_buf = buf.clone();
            let subscriber = tracing_subscriber::fmt()
                .with_max_level(tracing::Level::TRACE)
                .with_ansi(false)
                .without_time()
                .with_writer(move || Writer(writer_buf.clone()))
                .finish();
            let guard = tracing::subscriber::set_default(subscriber);
            Self { buf, _guard: guard }
        }

        pub fn logs(&self) -> String {
            String::from_utf8(self.buf.lock().expect("lock").clone()).expect("utf8")
        }

        pub fn count_substr(&self, needle: &str) -> usize {
            self.logs().matches(needle).count()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operator_log::events::{
        DAEMON_CYCLE_SUMMARY, DEXIE_OFFERS_ERROR, OFFER_POST_FAILURE,
    };
    use serde_json::json;

    #[test]
    fn audit_and_trace_redacts_offer_text_in_structured_mirror() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("greenfloor.sqlite")).expect("open");
        let capture = trace_capture::TraceCapture::install();
        let secret_tail = "z".repeat(64);
        let secret_offer = format!("offer1{secret_tail}");
        let payload = json!({
            "market_id": "m1",
            "offer_text": secret_offer.clone(),
            "error": "dexie_http_error:500",
        });

        audit_and_trace(
            &store,
            Level::WARN,
            LogContext::OFFER_POST,
            OFFER_POST_FAILURE,
            &payload,
            Some("m1"),
            "offer post failed",
        )
        .expect("audit");

        let events = store
            .list_recent_audit_events(Some(&[OFFER_POST_FAILURE]), Some("m1"), 1)
            .expect("events");
        assert_eq!(events.len(), 1);
        let stored = events[0].payload.get("offer_text").and_then(Value::as_str);
        assert_eq!(stored, Some(secret_offer.as_str()));

        let logs = capture.logs();
        assert!(!logs.contains(&secret_tail));
        assert!(!logs.contains("payload="));
        assert!(logs.contains("dexie_http_error:500"));
    }

    #[test]
    fn audit_market_cycle_dual_emits_audit_row_and_trace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("greenfloor.sqlite")).expect("open");
        let capture = trace_capture::TraceCapture::install();
        let payload = json!({
            "market_id": "m1",
            "error": "dexie_http_error:timeout",
        });

        audit_market_cycle(
            &store,
            Level::WARN,
            DEXIE_OFFERS_ERROR,
            &payload,
            "m1",
            "dexie offers fetch failed",
        )
        .expect("audit");

        let events = store
            .list_recent_audit_events(Some(&[DEXIE_OFFERS_ERROR]), Some("m1"), 1)
            .expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].payload.get("error").and_then(Value::as_str),
            Some("dexie_http_error:timeout")
        );
        let logs = capture.logs();
        assert!(logs.contains(DEXIE_OFFERS_ERROR));
        assert!(logs.contains("dexie_http_error:timeout"));
        assert!(!logs.contains("payload="));
    }

    #[test]
    fn audit_daemon_cycle_only_persists_without_trace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("greenfloor.sqlite")).expect("open");
        let capture = trace_capture::TraceCapture::install();
        let payload = json!({"error_count": 1});

        audit_daemon_cycle_only(&store, DAEMON_CYCLE_SUMMARY, &payload).expect("audit");

        let events = store
            .list_recent_audit_events(Some(&[DAEMON_CYCLE_SUMMARY]), None, 1)
            .expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(capture.count_substr(DAEMON_CYCLE_SUMMARY), 0);
    }

    #[test]
    fn trace_audit_outcome_emits_redacted_payload_without_persisting() {
        let capture = trace_capture::TraceCapture::install();
        let payload = json!({
            "market_id": "m1",
            "error": "dexie_http_error:500",
        });
        trace_audit_outcome(
            Level::WARN,
            LogContext::OFFER_POST,
            OFFER_POST_FAILURE,
            &payload,
            Some("m1"),
            "offer post failed",
        );
        let logs = capture.logs();
        assert!(logs.contains(OFFER_POST_FAILURE));
        assert!(logs.contains("dexie_http_error:500"));
    }
}
