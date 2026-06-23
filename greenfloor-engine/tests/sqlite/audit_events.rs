use serde_json::json;

use crate::common::open_store;

#[test]
fn sqlite_audit_insert() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("greenfloor.sqlite"));
    store
        .add_audit_event("test_event", &json!({"ok": true}), Some("m1"))
        .expect("insert audit");
}

#[test]
fn list_recent_audit_events_filters_by_event_type_and_market() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("greenfloor.sqlite"));
    store
        .add_audit_event("strategy_offer_execution", &json!({"id": 1}), Some("m1"))
        .expect("audit 1");
    store
        .add_audit_event("offer_reconciliation", &json!({"id": 2}), Some("m1"))
        .expect("audit 2");
    store
        .add_audit_event("offer_reconciliation", &json!({"id": 3}), Some("m2"))
        .expect("audit 3");
    let filtered = store
        .list_recent_audit_events(Some(&["offer_reconciliation"]), Some("m1"), 10)
        .expect("filtered");
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].event_type, "offer_reconciliation");
    assert_eq!(filtered[0].market_id.as_deref(), Some("m1"));
    assert_eq!(filtered[0].payload["id"], json!(2));
}

#[test]
fn list_recent_audit_events_non_positive_limit_returns_empty() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("greenfloor.sqlite"));
    store
        .add_audit_event("strategy_offer_execution", &json!({"id": 1}), Some("m1"))
        .expect("audit");
    assert!(store
        .list_recent_audit_events(None, None, 0)
        .expect("zero limit")
        .is_empty());
}

#[test]
fn recent_audit_payload_matches_finds_field_value() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("greenfloor.sqlite"));
    store
        .add_audit_event("config_reloaded", &json!({"reload_id": "reload-42"}), None)
        .expect("audit");
    assert!(store
        .recent_audit_payload_matches("config_reloaded", "reload_id", "reload-42", 5)
        .expect("match"));
    assert!(!store
        .recent_audit_payload_matches("config_reloaded", "reload_id", "missing", 5)
        .expect("no match"));
}
