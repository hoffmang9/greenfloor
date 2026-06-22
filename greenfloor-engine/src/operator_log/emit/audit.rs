use serde_json::Value;
use tracing::Level;

use crate::error::SignerResult;
use crate::operator_log::redact::redact_json_for_log;
use crate::storage::SqliteStore;

use super::context::{AuditDurability, LogContext};

pub(crate) fn persist_only(
    store: &SqliteStore,
    audit_event_type: &str,
    payload: &Value,
    market_id: Option<&str>,
    durability: AuditDurability,
) -> SignerResult<()> {
    match store.add_audit_event(audit_event_type, payload, market_id) {
        Ok(()) => Ok(()),
        Err(err) if durability == AuditDurability::Required => Err(err),
        Err(err) => {
            tracing::warn!(
                event = audit_event_type,
                error = %err,
                "operator audit persist failed"
            );
            Ok(())
        }
    }
}

pub(crate) fn persist_and_mirror(
    store: &SqliteStore,
    ctx: LogContext,
    level: Level,
    trace_message: &'static str,
    audit_event_type: &str,
    payload: &Value,
    market_id: Option<&str>,
) -> SignerResult<()> {
    persist_only(
        store,
        audit_event_type,
        payload,
        market_id,
        AuditDurability::Required,
    )?;
    trace_payload_mirror(
        level,
        ctx,
        audit_event_type,
        payload,
        market_id,
        trace_message,
    );
    Ok(())
}

pub(crate) fn mirror_only(
    ctx: LogContext,
    level: Level,
    trace_message: &'static str,
    audit_event_type: &str,
    payload: &Value,
    market_id: Option<&str>,
) {
    trace_payload_mirror(
        level,
        ctx,
        audit_event_type,
        payload,
        market_id,
        trace_message,
    );
}

pub(crate) fn trace_payload_mirror(
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

        LogContext::OFFER_POST
            .dual_audit(
                &store,
                Level::WARN,
                "offer post failed",
                OFFER_POST_FAILURE,
                &payload,
                Some("m1"),
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
    fn dual_audit_emits_audit_row_and_trace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("greenfloor.sqlite")).expect("open");
        let capture = TraceCapture::install();
        let payload = json!({
            "market_id": "m1",
            "error": "dexie_http_error:timeout",
        });

        LogContext::MARKET_CYCLE
            .dual_audit(
                &store,
                Level::WARN,
                "dexie offers fetch failed",
                DEXIE_OFFERS_ERROR,
                &payload,
                Some("m1"),
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
    fn audit_persists_without_trace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("greenfloor.sqlite")).expect("open");
        let capture = TraceCapture::install();
        let payload = json!({"error_count": 1});

        LogContext::DAEMON_CYCLE
            .audit(&store, DAEMON_CYCLE_SUMMARY, &payload, None)
            .expect("audit");

        let events = store
            .list_recent_audit_events(Some(&[DAEMON_CYCLE_SUMMARY]), None, 1)
            .expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(capture.count_substr(DAEMON_CYCLE_SUMMARY), 0);
    }

    #[test]
    fn dual_trace_emits_redacted_payload_without_persist() {
        let capture = TraceCapture::install();
        let payload = json!({"market_id": "m1", "error": "dexie_http_error:500"});
        LogContext::OFFER_POST
            .dual_trace(
                Level::WARN,
                "offer post failed",
                OFFER_POST_FAILURE,
                &payload,
                Some("m1"),
            )
            .expect("trace only");
        let logs = capture.logs();
        assert!(logs.contains(OFFER_POST_FAILURE));
        assert!(logs.contains("payload="));
    }
}
