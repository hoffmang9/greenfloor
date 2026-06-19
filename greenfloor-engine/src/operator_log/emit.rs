use serde_json::Value;
use tracing::Level;

use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::redact::redact_json_for_log;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuditDurability {
    #[default]
    Required,
    BestEffort,
}

#[derive(Debug, Clone, Copy)]
pub enum EmitMode {
    Dual {
        level: Level,
        trace_message: &'static str,
    },
    AuditOnly,
}

impl EmitMode {
    #[must_use]
    pub const fn dual(level: Level, trace_message: &'static str) -> Self {
        Self::Dual {
            level,
            trace_message,
        }
    }
}

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
    pub const COINSET: Self = Self {
        service: "daemon",
        phase: "coinset",
    };
}

impl LogContext {
    /// Persist and mirror one audit outcome (`AuditDurability::Required`).
    ///
    /// # Errors
    ///
    /// Returns an error when the audit insert fails.
    pub fn dual_audit(
        self,
        store: &SqliteStore,
        level: Level,
        trace_message: &'static str,
        audit_event_type: &str,
        payload: &Value,
        market_id: Option<&str>,
    ) -> SignerResult<()> {
        operator_audit(
            Some(store),
            self,
            EmitMode::dual(level, trace_message),
            audit_event_type,
            payload,
            market_id,
            AuditDurability::Required,
        )
    }

    /// Mirror one audit outcome to trace without persisting.
    ///
    /// # Errors
    ///
    /// Always returns `Ok(())` today; reserved for future trace-side failures.
    pub fn dual_trace(
        self,
        level: Level,
        trace_message: &'static str,
        audit_event_type: &str,
        payload: &Value,
        market_id: Option<&str>,
    ) -> SignerResult<()> {
        operator_audit(
            None,
            self,
            EmitMode::dual(level, trace_message),
            audit_event_type,
            payload,
            market_id,
            AuditDurability::BestEffort,
        )
    }

    /// Persist an audit row without a trace mirror (`AuditDurability::Required`).
    ///
    /// # Errors
    ///
    /// Returns an error when the audit insert fails.
    pub fn audit(
        self,
        store: &SqliteStore,
        audit_event_type: &str,
        payload: &Value,
        market_id: Option<&str>,
    ) -> SignerResult<()> {
        self.audit_with(
            store,
            audit_event_type,
            payload,
            market_id,
            AuditDurability::Required,
        )
    }

    /// Persist an audit row without a trace mirror.
    ///
    /// # Errors
    ///
    /// Returns an error when the audit insert fails.
    pub fn audit_with(
        self,
        store: &SqliteStore,
        audit_event_type: &str,
        payload: &Value,
        market_id: Option<&str>,
        durability: AuditDurability,
    ) -> SignerResult<()> {
        audit_row(
            store,
            self,
            audit_event_type,
            payload,
            market_id,
            durability,
        )
    }
}

#[derive(Debug, Clone)]
pub struct DeferredDualTrace {
    ctx: LogContext,
    level: Level,
    event: String,
    payload: Value,
    market_id: Option<String>,
    message: &'static str,
}

pub struct DeferredDualAudit {
    pub ctx: LogContext,
    pub level: Level,
    pub trace_message: &'static str,
    pub audit_event_type: &'static str,
    pub payload: Value,
    pub market_id: Option<String>,
}

/// Persist an audit row during a transaction and queue its trace mirror for after commit.
///
/// # Errors
///
/// Returns an error when the audit insert fails.
pub fn audit_row_defer_dual(
    deferred: &mut Vec<DeferredDualTrace>,
    store: &SqliteStore,
    entry: DeferredDualAudit,
) -> SignerResult<()> {
    audit_row(
        store,
        entry.ctx,
        entry.audit_event_type,
        &entry.payload,
        entry.market_id.as_deref(),
        AuditDurability::Required,
    )?;
    deferred.push(DeferredDualTrace {
        ctx: entry.ctx,
        level: entry.level,
        event: entry.audit_event_type.to_string(),
        payload: entry.payload,
        market_id: entry.market_id,
        message: entry.trace_message,
    });
    Ok(())
}

pub fn emit_deferred_dual_traces(deferred: &[DeferredDualTrace]) {
    for trace in deferred {
        trace_audit_mirror(
            trace.level,
            trace.ctx,
            &trace.event,
            &trace.payload,
            trace.market_id.as_deref(),
            trace.message,
        );
    }
}

pub fn trace_audit_mirror(
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

/// Persist (optional) and optionally mirror one audit outcome to trace.
///
/// # Errors
///
/// Returns an error when `AuditDurability::Required` and the audit insert fails.
pub fn operator_audit(
    store: Option<&SqliteStore>,
    ctx: LogContext,
    mode: EmitMode,
    audit_event_type: &str,
    payload: &Value,
    market_id: Option<&str>,
    durability: AuditDurability,
) -> SignerResult<()> {
    if let Some(store) = store {
        match store.add_audit_event(audit_event_type, payload, market_id) {
            Ok(()) => {}
            Err(err) => {
                if durability == AuditDurability::Required {
                    return Err(err);
                }
                tracing::warn!(
                    event = audit_event_type,
                    error = %err,
                    "operator audit persist failed"
                );
            }
        }
    }

    if let EmitMode::Dual {
        level,
        trace_message,
    } = mode
    {
        trace_audit_mirror(
            level,
            ctx,
            audit_event_type,
            payload,
            market_id,
            trace_message,
        );
    }
    Ok(())
}

/// Persist an audit row without a trace mirror.
///
/// # Errors
///
/// Returns an error when the audit insert fails.
pub fn audit_row(
    store: &SqliteStore,
    ctx: LogContext,
    audit_event_type: &str,
    payload: &Value,
    market_id: Option<&str>,
    durability: AuditDurability,
) -> SignerResult<()> {
    operator_audit(
        Some(store),
        ctx,
        EmitMode::AuditOnly,
        audit_event_type,
        payload,
        market_id,
        durability,
    )
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

/// Emit a structured operator trace event (`service`, `event`, and `phase` are always set).
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
mod tests {
    use super::*;
    use crate::operator_log::events::{
        DAEMON_CYCLE_SUMMARY, DEXIE_OFFERS_ERROR, OFFER_POST_FAILURE,
    };
    use crate::operator_log::test_util::TraceCapture;
    use serde_json::json;

    #[test]
    fn dual_emit_redacts_offer_text_in_blob_mirror() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("greenfloor.sqlite")).expect("open");
        let capture = TraceCapture::install();
        let secret_tail = "z".repeat(64);
        let secret_offer = format!("offer1{secret_tail}");
        let payload = json!({
            "market_id": "m1",
            "offer_text": secret_offer.clone(),
            "error": "dexie_http_error:500",
        });

        operator_audit(
            Some(&store),
            LogContext::OFFER_POST,
            EmitMode::dual(Level::WARN, "offer post failed"),
            OFFER_POST_FAILURE,
            &payload,
            Some("m1"),
            AuditDurability::Required,
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
        assert!(logs.contains("payload="));
        assert!(logs.contains("dexie_http_error:500"));
    }

    #[test]
    fn operator_audit_dual_emits_audit_row_and_trace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("greenfloor.sqlite")).expect("open");
        let capture = TraceCapture::install();
        let payload = json!({
            "market_id": "m1",
            "error": "dexie_http_error:timeout",
        });

        operator_audit(
            Some(&store),
            LogContext::MARKET_CYCLE,
            EmitMode::dual(Level::WARN, "dexie offers fetch failed"),
            DEXIE_OFFERS_ERROR,
            &payload,
            Some("m1"),
            AuditDurability::Required,
        )
        .expect("audit");

        let events = store
            .list_recent_audit_events(Some(&[DEXIE_OFFERS_ERROR]), Some("m1"), 1)
            .expect("events");
        assert_eq!(events.len(), 1);
        let logs = capture.logs();
        assert!(logs.contains(DEXIE_OFFERS_ERROR));
        assert!(logs.contains("payload="));
    }

    #[test]
    fn audit_row_persists_without_trace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("greenfloor.sqlite")).expect("open");
        let capture = TraceCapture::install();
        let payload = json!({"error_count": 1});

        audit_row(
            &store,
            LogContext::DAEMON_CYCLE,
            DAEMON_CYCLE_SUMMARY,
            &payload,
            None,
            AuditDurability::Required,
        )
        .expect("audit");

        let events = store
            .list_recent_audit_events(Some(&[DAEMON_CYCLE_SUMMARY]), None, 1)
            .expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(capture.count_substr(DAEMON_CYCLE_SUMMARY), 0);
    }

    #[test]
    fn best_effort_persist_failure_still_traces_dual_emit() {
        let capture = TraceCapture::install();
        let payload = json!({"market_id": "m1", "error": "dexie_http_error:500"});
        operator_audit(
            None,
            LogContext::OFFER_POST,
            EmitMode::dual(Level::WARN, "offer post failed"),
            OFFER_POST_FAILURE,
            &payload,
            Some("m1"),
            AuditDurability::BestEffort,
        )
        .expect("trace only");
        let logs = capture.logs();
        assert!(logs.contains(OFFER_POST_FAILURE));
        assert!(logs.contains("payload="));
    }
}
