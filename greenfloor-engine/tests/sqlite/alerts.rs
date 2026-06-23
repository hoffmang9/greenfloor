use greenfloor_engine::storage::StoredAlertState;

use crate::common::open_store;

#[test]
fn sqlite_alert_state_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("greenfloor.sqlite"));
    let original = store.get_alert_state("m1").expect("get alert");
    assert!(!original.is_low);
    store
        .upsert_alert_state(&StoredAlertState {
            market_id: "m1".to_string(),
            is_low: true,
            last_alert_at: None,
        })
        .expect("upsert alert");
    let got = store.get_alert_state("m1").expect("get alert");
    assert!(got.is_low);
}

#[test]
fn sqlite_alert_state_persists_last_alert_at() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("greenfloor.sqlite"));
    store
        .upsert_alert_state(&StoredAlertState {
            market_id: "m2".to_string(),
            is_low: true,
            last_alert_at: Some("2020-01-01T00:00:00Z".to_string()),
        })
        .expect("upsert alert");
    let got = store.get_alert_state("m2").expect("get alert");
    assert!(got.is_low);
    assert_eq!(got.last_alert_at.as_deref(), Some("2020-01-01T00:00:00Z"));
}
