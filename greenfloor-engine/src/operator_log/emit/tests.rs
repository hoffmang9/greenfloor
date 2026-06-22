use serde_json::Value;
use tracing::Level;

use crate::operator_log::events::{DAEMON_CYCLE_SUMMARY, DEXIE_OFFERS_ERROR, OFFER_POST_FAILURE};
use crate::operator_log::test_util::TraceCapture;
use crate::operator_log::LogContext;
use crate::storage::SqliteStore;
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
    LogContext::OFFER_POST.dual_trace(
        Level::WARN,
        "offer post failed",
        OFFER_POST_FAILURE,
        &payload,
        Some("m1"),
    );
    let logs = capture.logs();
    assert!(logs.contains(OFFER_POST_FAILURE));
    assert!(logs.contains("payload="));
}
