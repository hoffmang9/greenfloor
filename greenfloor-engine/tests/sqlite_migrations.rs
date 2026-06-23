use greenfloor_engine::hex::legacy_prefixed_tx_id;
use greenfloor_engine::storage::test_support::open_pre_migration_connection;
use greenfloor_engine::storage::SqliteStore;
use rusqlite::{params, Connection};
use std::path::Path;

#[derive(Copy, Clone)]
struct MigrationCase {
    name: &'static str,
    seed: fn(&Connection),
    assert: fn(&SqliteStore, &Path),
}

fn run_migration_case(case: MigrationCase) {
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("{}: tempdir: {err}", case.name));
    let path = dir.path().join("migrate.db");
    {
        let conn = open_pre_migration_connection(&path)
            .unwrap_or_else(|err| panic!("{}: pre-migration open: {err}", case.name));
        (case.seed)(&conn);
    }
    let store =
        SqliteStore::open(&path).unwrap_or_else(|err| panic!("{}: open store: {err}", case.name));
    (case.assert)(&store, &path);
}

fn seed_legacy_tx_signal(conn: &Connection) {
    let canonical = "a".repeat(64);
    let legacy = legacy_prefixed_tx_id(&canonical).expect("legacy id");
    conn.execute(
        "INSERT INTO tx_signal_state (tx_id, mempool_observed_at) VALUES (?1, ?2)",
        params![legacy, "2020-01-01T00:00:00Z"],
    )
    .expect("insert legacy tx id");
}

fn assert_legacy_tx_signal_normalized(store: &SqliteStore, path: &Path) {
    let canonical = "a".repeat(64);
    let legacy = legacy_prefixed_tx_id(&canonical).expect("legacy id");
    let state = store
        .get_tx_signal_state(std::slice::from_ref(&canonical))
        .expect("lookup canonical");
    assert!(state.contains_key(&canonical));
    let conn = Connection::open(path).expect("reopen");
    let legacy_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tx_signal_state WHERE tx_id = ?1",
            params![legacy],
            |row| row.get(0),
        )
        .expect("count legacy");
    assert_eq!(legacy_count, 0);
}

fn seed_legacy_offer_cancel_tx_id(conn: &Connection) {
    let canonical = "b".repeat(64);
    let legacy = legacy_prefixed_tx_id(&canonical).expect("legacy id");
    conn.execute(
        r"
        INSERT INTO offer_state
          (offer_id, market_id, state, updated_at, cancel_submitted_tx_id)
        VALUES (?1, ?2, ?3, ?4, ?5)
        ",
        params![
            "offer-1",
            "m1",
            "cancel_submitted",
            "2020-01-01T00:00:00Z",
            legacy
        ],
    )
    .expect("insert legacy cancel tx id");
}

fn assert_legacy_offer_cancel_tx_id_normalized(store: &SqliteStore, _path: &Path) {
    let canonical = "b".repeat(64);
    let row = store
        .list_offer_states_for_ids(&["offer-1".to_string()])
        .expect("rows")
        .into_iter()
        .next()
        .expect("row");
    assert_eq!(
        row.cancel_submitted_tx_id.as_deref(),
        Some(canonical.as_str())
    );
}

fn seed_cancel_submitted_without_timestamp(conn: &Connection) {
    conn.execute(
        r"
        INSERT INTO offer_state
          (offer_id, market_id, state, updated_at, cancel_submitted_at)
        VALUES (?1, ?2, ?3, ?4, NULL)
        ",
        params!["offer-1", "m1", "cancel_submitted", "2020-01-01T00:00:00Z"],
    )
    .expect("insert cancel_submitted without timestamp");
}

fn assert_cancel_submitted_at_backfilled(store: &SqliteStore, _path: &Path) {
    let row = store
        .list_offer_states_for_ids(&["offer-1".to_string()])
        .expect("rows")
        .into_iter()
        .next()
        .expect("row");
    assert_eq!(
        row.cancel_submitted_at.as_deref(),
        Some("2020-01-01T00:00:00Z")
    );
}

#[test]
fn normalize_legacy_tx_signal_ids_on_store_open() {
    run_migration_case(MigrationCase {
        name: "normalize_legacy_tx_signal_ids",
        seed: seed_legacy_tx_signal,
        assert: assert_legacy_tx_signal_normalized,
    });
}

#[test]
fn normalize_legacy_offer_cancel_tx_id_on_store_open() {
    run_migration_case(MigrationCase {
        name: "normalize_legacy_offer_cancel_tx_id",
        seed: seed_legacy_offer_cancel_tx_id,
        assert: assert_legacy_offer_cancel_tx_id_normalized,
    });
}

#[test]
fn backfill_cancel_submitted_at_from_updated_at_on_store_open() {
    run_migration_case(MigrationCase {
        name: "backfill_cancel_submitted_at_from_updated_at",
        seed: seed_cancel_submitted_without_timestamp,
        assert: assert_cancel_submitted_at_backfilled,
    });
}
