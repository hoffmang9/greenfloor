use crate::common::{coin_op_entry, open_store, raw_conn};

#[test]
fn sqlite_daily_fee_spent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("greenfloor.sqlite"));
    store
        .add_coin_op_ledger_entry(&coin_op_entry(
            "m1",
            "split",
            1,
            10,
            "executed",
            "stub_executed",
            Some("op-1"),
        ))
        .expect("executed entry");
    store
        .add_coin_op_ledger_entry(&coin_op_entry(
            "m1",
            "combine",
            1,
            99,
            "skipped",
            "fee_budget_guard",
            None,
        ))
        .expect("skipped entry");
    assert_eq!(
        store.get_daily_fee_spent_mojos_utc().expect("daily fee"),
        10
    );
}

#[test]
fn coin_op_budget_report() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("greenfloor.sqlite"));
    store
        .add_coin_op_ledger_entry(&coin_op_entry(
            "m1",
            "split",
            2,
            20,
            "executed",
            "stub_executed",
            Some("op-1"),
        ))
        .expect("executed");
    store
        .add_coin_op_ledger_entry(&coin_op_entry(
            "m1",
            "split",
            3,
            0,
            "planned",
            "dry_run",
            Some("dryrun-1"),
        ))
        .expect("planned");
    store
        .add_coin_op_ledger_entry(&coin_op_entry(
            "m1",
            "combine",
            4,
            0,
            "skipped",
            "fee_budget_guard",
            None,
        ))
        .expect("skipped");
    let report = store.get_coin_op_budget_report_utc().expect("report");
    assert_eq!(report.spent_mojos, 20);
    assert_eq!(report.executed_ops, 2);
    assert_eq!(report.planned_ops, 3);
    assert_eq!(report.skipped_ops, 4);
    assert_eq!(report.fee_budget_skipped_ops, 4);
}

#[test]
fn add_coin_op_ledger_entry_persists_all_fields() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("gf.sqlite");
    let store = open_store(&db_path);
    store
        .add_coin_op_ledger_entry(&coin_op_entry(
            "m1",
            "split",
            3,
            500,
            "executed",
            "normal",
            Some("op-abc"),
        ))
        .expect("insert");
    let row = raw_conn(&db_path)
        .query_row(
            "SELECT market_id, op_type, op_count, fee_mojos, status, reason, operation_id FROM coin_op_ledger ORDER BY id DESC LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            },
        )
        .expect("row");
    assert_eq!(row.0, "m1");
    assert_eq!(row.1, "split");
    assert_eq!(row.2, 3);
    assert_eq!(row.3, 500);
    assert_eq!(row.4, "executed");
    assert_eq!(row.5, "normal");
    assert_eq!(row.6.as_deref(), Some("op-abc"));
}

#[test]
fn add_coin_op_ledger_entry_null_operation_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("gf.sqlite");
    let store = open_store(&db_path);
    store
        .add_coin_op_ledger_entry(&coin_op_entry(
            "m1", "combine", 1, 0, "skipped", "dry_run", None,
        ))
        .expect("insert");
    let operation_id: Option<String> = raw_conn(&db_path)
        .query_row(
            "SELECT operation_id FROM coin_op_ledger ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("operation_id");
    assert!(operation_id.is_none());
}
