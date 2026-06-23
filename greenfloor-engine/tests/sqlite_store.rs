use std::collections::BTreeMap;

use chrono::{Duration, Utc};
use greenfloor_engine::storage::{
    CoinOpLedgerEntry, OfferReservationAcquireOutcome, OfferReservationLeaseRequest,
    OfferReservationRejectReason, SqliteStore, StoredAlertState,
};
use rusqlite::Connection;
use serde_json::json;

fn open_store(path: &std::path::Path) -> SqliteStore {
    SqliteStore::open(path).expect("open sqlite store")
}

fn acquire_test_reservation_lease(
    store: &SqliteStore,
    reservation_id: &str,
    wallet_id: &str,
    amounts: &BTreeMap<String, i64>,
    lease_seconds: i64,
) {
    assert!(
        matches!(
            store
                .try_acquire_offer_reservation_lease(&OfferReservationLeaseRequest {
                    reservation_id,
                    market_id: "m1",
                    wallet_id,
                    requested_amounts: amounts,
                    available_amounts: amounts,
                    lease_seconds,
                    now: None,
                })
                .expect("try acquire"),
            OfferReservationAcquireOutcome::Acquired
        ),
        "reservation acquire failed for {reservation_id}"
    );
}

fn coin_op_entry<'a>(
    market_id: &'a str,
    op_type: &'a str,
    op_count: i64,
    fee_mojos: i64,
    status: &'a str,
    reason: &'a str,
    operation_id: Option<&'a str>,
) -> CoinOpLedgerEntry<'a> {
    CoinOpLedgerEntry {
        market_id,
        op_type,
        op_count,
        fee_mojos,
        status,
        reason,
        operation_id,
    }
}

fn raw_conn(path: &std::path::Path) -> Connection {
    Connection::open(path).expect("open raw sqlite connection")
}

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
fn sqlite_audit_insert() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("greenfloor.sqlite"));
    store
        .add_audit_event("test_event", &json!({"ok": true}), Some("m1"))
        .expect("insert audit");
}

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
fn get_tx_signal_state_dedupes_and_ignores_empty_ids() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("greenfloor.sqlite"));
    let tx_a = "a".repeat(64);
    let tx_b = "b".repeat(64);
    assert_eq!(
        store
            .observe_mempool_tx_ids(&[tx_a.clone(), tx_b.clone()])
            .expect("observe"),
        2
    );
    assert_eq!(
        store
            .confirm_tx_ids(&[tx_a.clone()])
            .expect("confirm"),
        1
    );
    let state = store
        .get_tx_signal_state(&[
            tx_a.clone(),
            String::new(),
            "   ".to_string(),
            tx_a.clone(),
            tx_b.clone(),
            "c".repeat(64),
        ])
        .expect("state");
    assert_eq!(state.len(), 2);
    assert!(state[&tx_a].mempool_observed_at.is_some());
    assert!(state[&tx_a].tx_block_confirmed_at.is_some());
    assert!(state[&tx_b].mempool_observed_at.is_some());
    assert!(state[&tx_b].tx_block_confirmed_at.is_none());
}

#[test]
fn get_tx_signal_state_returns_empty_for_no_usable_ids() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("greenfloor.sqlite"));
    assert!(store.get_tx_signal_state(&[]).expect("empty").is_empty());
    assert!(store
        .get_tx_signal_state(&[String::new(), "   ".to_string()])
        .expect("blank ids")
        .is_empty());
}

#[test]
fn tx_signal_state_normalizes_prefixed_tx_ids() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("greenfloor.sqlite"));
    let tx_id = "a".repeat(64);
    assert_eq!(
        store
            .observe_mempool_tx_ids(&[format!("0x{tx_id}")])
            .expect("observe"),
        1
    );
    let state = store
        .get_tx_signal_state(&[tx_id.clone()])
        .expect("state");
    assert!(state.get(&tx_id).is_some_and(|row| row.mempool_observed_at.is_some()));
}

#[test]
fn confirm_tx_ids_updates_legacy_prefixed_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let store = open_store(&db_path);
    let tx_id = "e".repeat(64);
    raw_conn(&db_path)
        .execute(
            "INSERT INTO tx_signal_state (tx_id, mempool_observed_at) VALUES (?1, ?2)",
            rusqlite::params![format!("0x{tx_id}"), "2020-01-01T00:00:00Z"],
        )
        .expect("seed legacy row");
    assert_eq!(store.confirm_tx_ids(&[tx_id.clone()]).expect("confirm"), 1);
    let state = store
        .get_tx_signal_state(&[tx_id.clone()])
        .expect("state");
    assert!(state
        .get(&tx_id)
        .and_then(|row| row.tx_block_confirmed_at.as_deref())
        .is_some());
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
fn upsert_offer_state_insert_and_list() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    store
        .upsert_offer_state("offer-1", "m1", "open", Some(4))
        .expect("upsert");
    let rows = store.list_offer_states(None, 10).expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].offer_id, "offer-1");
    assert_eq!(rows[0].market_id, "m1");
    assert_eq!(rows[0].state, "open");
    assert_eq!(rows[0].last_seen_status, Some(4));
}

#[test]
fn upsert_offer_state_updates_existing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    store
        .upsert_offer_state("offer-1", "m1", "open", Some(4))
        .expect("upsert open");
    store
        .upsert_offer_state("offer-1", "m1", "expired", Some(6))
        .expect("upsert expired");
    let rows = store.list_offer_states(None, 10).expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, "expired");
    assert_eq!(rows[0].last_seen_status, Some(6));
}

#[test]
fn upsert_offer_state_null_last_seen_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    store
        .upsert_offer_state("offer-2", "m1", "unknown", None)
        .expect("upsert");
    let rows = store.list_offer_states(None, 10).expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].last_seen_status, None);
}

#[test]
fn list_offer_states_filters_by_market_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    store
        .upsert_offer_state("o1", "m1", "open", Some(4))
        .expect("o1");
    store
        .upsert_offer_state("o2", "m2", "open", Some(4))
        .expect("o2");
    store
        .upsert_offer_state("o3", "m1", "expired", Some(6))
        .expect("o3");
    let m1_rows = store.list_offer_states(Some("m1"), 10).expect("m1");
    assert_eq!(m1_rows.len(), 2);
    assert!(m1_rows.iter().all(|row| row.market_id == "m1"));
    assert_eq!(
        store.list_offer_states(Some("m2"), 10).expect("m2").len(),
        1
    );
}

#[test]
fn list_offer_states_respects_limit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    for i in 0..5 {
        store
            .upsert_offer_state(&format!("o{i}"), "m1", "open", Some(4))
            .expect("seed");
    }
    assert_eq!(store.list_offer_states(None, 3).expect("limit 3").len(), 3);
    assert!(store
        .list_offer_states(None, 0)
        .expect("limit 0")
        .is_empty());
}

#[test]
fn list_offer_states_orders_by_updated_at_desc() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    store
        .upsert_offer_state_at("first", "m1", "open", Some(4), "2020-01-01T00:00:00Z")
        .expect("first");
    store
        .upsert_offer_state_at("second", "m1", "open", Some(4), "2021-01-01T00:00:00Z")
        .expect("second");
    let rows = store.list_offer_states(None, 10).expect("list");
    assert_eq!(rows[0].offer_id, "second");
    assert_eq!(rows[1].offer_id, "first");
}

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

#[test]
fn offer_reservation_lease_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    let mut amounts = BTreeMap::default();
    amounts.insert("xch".to_string(), 1000);
    amounts.insert("cat-1".to_string(), 2500);
    acquire_test_reservation_lease(&store, "res-1", "vault-1", &amounts, 120);
    let rows = store
        .list_offer_reservation_leases(Some("res-1"))
        .expect("list");
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows.iter()
            .map(|row| row.asset_id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        ["cat-1", "xch"].into_iter().collect()
    );
    let reserved = store
        .get_offer_reserved_amounts_by_asset("vault-1")
        .expect("reserved");
    assert_eq!(reserved.get("xch"), Some(&1000));
    assert_eq!(reserved.get("cat-1"), Some(&2500));
}

#[test]
fn offer_reservation_release_clears_reserved_amount() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    let mut amounts = BTreeMap::default();
    amounts.insert("xch".to_string(), 700);
    acquire_test_reservation_lease(&store, "res-2", "vault-1", &amounts, 120);
    assert_eq!(
        store
            .release_offer_reservation_lease("res-2", "released_success")
            .expect("release"),
        1
    );
    let reserved = store
        .get_offer_reserved_amounts_by_asset("vault-1")
        .expect("reserved");
    assert_eq!(reserved.get("xch").copied().unwrap_or(0), 0);
    let rows = store
        .list_offer_reservation_leases(Some("res-2"))
        .expect("rows");
    assert_eq!(rows[0].status, "released_success");
    assert!(rows[0].released_at.is_some());
}

#[test]
fn offer_reservation_expiry_marks_active_rows_expired() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    let mut amounts = BTreeMap::default();
    amounts.insert("xch".to_string(), 120);
    acquire_test_reservation_lease(&store, "res-3", "vault-1", &amounts, 1);
    assert_eq!(
        store
            .expire_offer_reservation_leases(Some(Utc::now() + Duration::hours(1)))
            .expect("expire"),
        1
    );
    let rows = store
        .list_offer_reservation_leases(Some("res-3"))
        .expect("rows");
    assert_eq!(rows[0].status, "expired");
    assert!(rows[0].released_at.is_some());
}

#[test]
fn try_acquire_offer_reservation_lease_rejects_insufficient_capacity() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    let mut requested = BTreeMap::default();
    requested.insert("asset".to_string(), 100);
    let mut available = BTreeMap::default();
    available.insert("asset".to_string(), 50);
    let outcome = store
        .try_acquire_offer_reservation_lease(&OfferReservationLeaseRequest {
            reservation_id: "res-4",
            market_id: "m1",
            wallet_id: "vault-1",
            requested_amounts: &requested,
            available_amounts: &available,
            lease_seconds: 120,
            now: None,
        })
        .expect("try acquire");
    let OfferReservationAcquireOutcome::Rejected(reason) = outcome else {
        panic!("expected rejection, got {outcome:?}");
    };
    assert_eq!(
        reason,
        OfferReservationRejectReason::InsufficientCapacity {
            asset_id: "asset".to_string(),
            available: 50,
            reserved: 0,
            needed: 100,
        }
    );
    assert!(store
        .list_offer_reservation_leases(Some("res-4"))
        .expect("rows")
        .is_empty());
}

#[test]
fn try_acquire_offer_reservation_lease_persists_rows_on_success() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    let mut requested = BTreeMap::default();
    requested.insert("asset".to_string(), 100);
    requested.insert("xch".to_string(), 20);
    let mut available = BTreeMap::default();
    available.insert("asset".to_string(), 150);
    available.insert("xch".to_string(), 40);
    assert!(matches!(
        store
            .try_acquire_offer_reservation_lease(&OfferReservationLeaseRequest {
                reservation_id: "res-5",
                market_id: "m1",
                wallet_id: "vault-1",
                requested_amounts: &requested,
                available_amounts: &available,
                lease_seconds: 120,
                now: None,
            })
            .expect("try acquire"),
        OfferReservationAcquireOutcome::Acquired
    ));
    assert_eq!(
        store
            .list_offer_reservation_leases(Some("res-5"))
            .expect("rows")
            .len(),
        2
    );
    let reserved = store
        .get_offer_reserved_amounts_by_asset("vault-1")
        .expect("reserved");
    assert_eq!(reserved.get("asset"), Some(&100));
    assert_eq!(reserved.get("xch"), Some(&20));
}

#[test]
fn prune_offer_reservation_leases_removes_old_inactive_rows() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(&dir.path().join("gf.sqlite"));
    let mut amounts = BTreeMap::default();
    amounts.insert("asset".to_string(), 10);
    acquire_test_reservation_lease(&store, "res-6", "vault-1", &amounts, 120);
    store
        .release_offer_reservation_lease("res-6", "released_success")
        .expect("release");
    assert_eq!(
        store
            .prune_offer_reservation_leases(Utc::now() + Duration::hours(1))
            .expect("prune"),
        1
    );
    assert!(store
        .list_offer_reservation_leases(Some("res-6"))
        .expect("rows")
        .is_empty());
}
