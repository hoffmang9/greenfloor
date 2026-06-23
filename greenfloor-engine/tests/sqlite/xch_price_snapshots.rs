use serde_json::json;

use crate::common::open_store;

#[test]
fn get_latest_xch_price_snapshot_none_when_empty() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    assert_eq!(store.get_latest_xch_price_snapshot().expect("latest"), None);
}

#[test]
fn get_latest_xch_price_snapshot_returns_latest() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    store
        .add_audit_event("xch_price_snapshot", &json!({"price_usd": 25.5}), None)
        .expect("first");
    store
        .add_audit_event("xch_price_snapshot", &json!({"price_usd": 30.0}), None)
        .expect("second");
    assert_eq!(
        store.get_latest_xch_price_snapshot().expect("latest"),
        Some(30.0)
    );
}

#[test]
fn get_latest_xch_price_snapshot_ignores_non_price_events() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    store
        .add_audit_event("other_event", &json!({"price_usd": 99.0}), None)
        .expect("other");
    assert_eq!(store.get_latest_xch_price_snapshot().expect("latest"), None);
}

#[test]
fn get_latest_xch_price_snapshot_rejects_non_positive() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    store
        .add_audit_event("xch_price_snapshot", &json!({"price_usd": 0}), None)
        .expect("zero");
    assert_eq!(store.get_latest_xch_price_snapshot().expect("latest"), None);
    store
        .add_audit_event("xch_price_snapshot", &json!({"price_usd": -5.0}), None)
        .expect("negative");
    assert_eq!(store.get_latest_xch_price_snapshot().expect("latest"), None);
}

#[test]
fn get_latest_xch_price_snapshot_handles_malformed_payload() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    store
        .add_audit_event("xch_price_snapshot", &json!({"no_price_key": true}), None)
        .expect("malformed");
    assert_eq!(store.get_latest_xch_price_snapshot().expect("latest"), None);
}
