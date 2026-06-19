use serde_json::Value;
use tracing::Level;

use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::redact::redact_json_for_log;

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

fn emit(level: Level, ctx: LogContext, event: &str, message: &str) {
    match level {
        Level::ERROR => tracing::error!(
            service = ctx.service,
            event = event,
            phase = ctx.phase,
            "{message}"
        ),
        Level::WARN => tracing::warn!(
            service = ctx.service,
            event = event,
            phase = ctx.phase,
            "{message}"
        ),
        Level::INFO => tracing::info!(
            service = ctx.service,
            event = event,
            phase = ctx.phase,
            "{message}"
        ),
        Level::DEBUG | Level::TRACE => tracing::debug!(
            service = ctx.service,
            event = event,
            phase = ctx.phase,
            "{message}"
        ),
    }
}

pub fn info(ctx: LogContext, event: &str, message: &str) {
    emit(Level::INFO, ctx, event, message);
}

pub fn warn(ctx: LogContext, event: &str, message: &str) {
    emit(Level::WARN, ctx, event, message);
}

pub fn error(ctx: LogContext, event: &str, message: &str) {
    emit(Level::ERROR, ctx, event, message);
}

pub fn debug(ctx: LogContext, event: &str, message: &str) {
    emit(Level::DEBUG, ctx, event, message);
}

/// Emit matching tracing + `SQLite` audit rows for operator-visible outcomes.
///
/// Audit payload is stored verbatim; tracing receives a redacted copy.
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
    trace_message: &str,
) -> SignerResult<()> {
    store.add_audit_event(audit_event_type, payload, market_id)?;
    let payload_text = redact_json_for_log(payload).to_string();
    trace_audit_payload(
        level,
        ctx,
        audit_event_type,
        market_id,
        &payload_text,
        trace_message,
    );
    Ok(())
}

fn trace_audit_payload(
    level: Level,
    ctx: LogContext,
    audit_event_type: &str,
    market_id: Option<&str>,
    payload_text: &str,
    trace_message: &str,
) {
    if level == Level::ERROR {
        tracing::error!(
            service = ctx.service,
            event = audit_event_type,
            phase = ctx.phase,
            market_id = market_id.unwrap_or(""),
            payload = %payload_text,
            "{trace_message}"
        );
    } else if level == Level::WARN {
        tracing::warn!(
            service = ctx.service,
            event = audit_event_type,
            phase = ctx.phase,
            market_id = market_id.unwrap_or(""),
            payload = %payload_text,
            "{trace_message}"
        );
    } else if level == Level::INFO {
        tracing::info!(
            service = ctx.service,
            event = audit_event_type,
            phase = ctx.phase,
            market_id = market_id.unwrap_or(""),
            payload = %payload_text,
            "{trace_message}"
        );
    } else {
        tracing::debug!(
            service = ctx.service,
            event = audit_event_type,
            phase = ctx.phase,
            market_id = market_id.unwrap_or(""),
            payload = %payload_text,
            "{trace_message}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operator_log::inventory::OFFER_POST_ITERATION;
    use serde_json::json;

    #[test]
    fn audit_and_trace_persists_redacted_safe_payload() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("greenfloor.sqlite")).expect("open");
        let secret_tail = "z".repeat(64);
        let secret_offer = format!("offer1{secret_tail}");
        let payload = json!({
            "market_id": "m1",
            "offer_text": secret_offer,
            "error": "dexie_http_error:500",
        });

        audit_and_trace(
            &store,
            Level::WARN,
            LogContext::OFFER_POST,
            "offer_post_failure",
            &payload,
            Some("m1"),
            "offer post iteration failed",
        )
        .expect("audit");

        let events = store
            .list_recent_audit_events(Some(&["offer_post_failure"]), Some("m1"), 1)
            .expect("events");
        assert_eq!(events.len(), 1);
        let stored = events[0].payload.get("offer_text").and_then(Value::as_str);
        assert_eq!(
            stored,
            Some(secret_offer.as_str()),
            "audit keeps full payload"
        );

        let redacted = crate::operator_log::redact_json_for_log(&payload);
        let redacted_offer = redacted
            .get("offer_text")
            .and_then(Value::as_str)
            .expect("redacted offer");
        assert!(!redacted_offer.contains(&secret_tail));
        assert!(redacted_offer.contains("len="));

        let _ = OFFER_POST_ITERATION;
    }
}
