use serde_json::json;

use crate::common::{open_store, raw_conn};

#[test]
fn add_price_policy_snapshot_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("gf.sqlite");
    let store = open_store(&db_path);
    store
        .add_price_policy_snapshot("m1", &json!({"spread_bps": 100}), "startup")
        .expect("startup");
    store
        .add_price_policy_snapshot("m1", &json!({"spread_bps": 200}), "update")
        .expect("update");
    let conn = raw_conn(&db_path);
    let mut stmt = conn
        .prepare("SELECT market_id, source, payload_json FROM price_policy_history ORDER BY id")
        .expect("prepare");
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].1, "startup");
    assert_eq!(rows[1].1, "update");
}
