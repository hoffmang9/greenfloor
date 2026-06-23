use greenfloor_engine::storage::StoredAlertState;

use crate::common::open_store;

#[test]
fn sqlite_alert_state_roundtrip() {
    for (market_id, last_alert_at, expected_last_alert_at) in [
        ("m1", None, None),
        (
            "m2",
            Some("2020-01-01T00:00:00Z"),
            Some("2020-01-01T00:00:00Z"),
        ),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_store(&dir.path().join("greenfloor.sqlite"));
        if market_id == "m1" {
            let original = store.get_alert_state(market_id).expect("get alert");
            assert!(!original.is_low);
        }
        store
            .upsert_alert_state(&StoredAlertState {
                market_id: market_id.to_string(),
                is_low: true,
                last_alert_at: last_alert_at.map(str::to_string),
            })
            .expect("upsert alert");
        let got = store.get_alert_state(market_id).expect("get alert");
        assert!(got.is_low);
        assert_eq!(got.last_alert_at.as_deref(), expected_last_alert_at);
    }
}
