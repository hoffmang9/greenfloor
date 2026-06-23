use chrono::{Duration, Utc};
use greenfloor_engine::operator_log::{
    COIN_OPS_EXECUTED, COIN_OP_LEDGER_EXECUTED, OFFER_CANCEL_POLICY, OFFER_LIFECYCLE_TRANSITION,
    TAKER_DETECTION,
};
use greenfloor_engine::storage::{
    is_preserved_audit_row, PruneAuditEventsOptions, SqliteStore, DEFAULT_AUDIT_RETENTION_DAYS,
};
use serde_json::json;
use tempfile::TempDir;

fn open_store(path: &std::path::Path) -> SqliteStore {
    SqliteStore::open(path).expect("open store")
}

fn seed_old_rows(store: &SqliteStore, created_at: &str) {
    store
        .add_audit_event_at(
            "daemon_cycle_summary",
            &json!({"ok": true}),
            None,
            created_at,
        )
        .expect("noise");
    store
        .add_audit_event_at(
            OFFER_LIFECYCLE_TRANSITION,
            &json!({"new_state":"open","reason":"ok"}),
            Some("m1"),
            created_at,
        )
        .expect("open transition");
    store
        .add_audit_event_at(
            OFFER_LIFECYCLE_TRANSITION,
            &json!({"new_state":"cancelled","reason":"cancel_tx_chain_confirmed"}),
            Some("m1"),
            created_at,
        )
        .expect("cancelled transition");
    store
        .add_audit_event_at(
            OFFER_LIFECYCLE_TRANSITION,
            &json!({"new_state":"mempool_observed","reason":"coinset_mempool_observed"}),
            Some("m1"),
            created_at,
        )
        .expect("take mempool transition");
    store
        .add_audit_event_at(
            "coin_op_executed",
            &json!({"operation_id":"abc"}),
            Some("m1"),
            created_at,
        )
        .expect("coin op");
    store
        .add_audit_event_at(
            OFFER_CANCEL_POLICY,
            &json!({"executed_count": 1, "items": []}),
            Some("m1"),
            created_at,
        )
        .expect("cancel policy");
}

#[test]
fn prune_stale_audit_events_dry_run_counts_without_deleting() {
    let dir = TempDir::new().expect("tempdir");
    let store = open_store(&dir.path().join("state.sqlite"));
    let old = (Utc::now() - Duration::days(40)).to_rfc3339();
    seed_old_rows(&store, &old);

    let report = store
        .prune_stale_audit_events(
            DEFAULT_AUDIT_RETENTION_DAYS,
            PruneAuditEventsOptions::cli(true, false),
        )
        .expect("dry run");
    assert!(report.dry_run);
    assert_eq!(report.deleted_count, 0);
    assert_eq!(report.deletable_count, 2);
    assert_eq!(
        store
            .list_recent_audit_events(None, None, 20)
            .expect("list")
            .len(),
        6
    );
}

#[test]
fn prune_stale_audit_events_deletes_only_old_non_financial_rows() {
    let dir = TempDir::new().expect("tempdir");
    let store = open_store(&dir.path().join("state.sqlite"));
    let old = (Utc::now() - Duration::days(40)).to_rfc3339();
    let recent = Utc::now().to_rfc3339();
    seed_old_rows(&store, &old);
    store
        .add_audit_event_at(
            "xch_price_snapshot",
            &json!({"price_usd": 1.0}),
            None,
            &recent,
        )
        .expect("recent noise");

    let report = store
        .prune_stale_audit_events(
            DEFAULT_AUDIT_RETENTION_DAYS,
            PruneAuditEventsOptions {
                dry_run: false,
                vacuum: false,
                batch_size: 1,
            },
        )
        .expect("prune");
    assert_eq!(report.deleted_count, 2);
    assert_eq!(
        store
            .list_recent_audit_events(None, None, 20)
            .expect("list")
            .len(),
        5
    );
}

#[test]
fn prune_stale_audit_events_vacuum_flag_runs_vacuum() {
    let dir = TempDir::new().expect("tempdir");
    let store = open_store(&dir.path().join("state.sqlite"));
    let old = (Utc::now() - Duration::days(40)).to_rfc3339();
    store
        .add_audit_event_at("daemon_cycle_summary", &json!({"ok": true}), None, &old)
        .expect("noise");

    let report = store
        .prune_stale_audit_events(30, PruneAuditEventsOptions::cli(false, true))
        .expect("prune");
    assert!(report.vacuum_ran);
    assert_eq!(report.deleted_count, 1);
}

#[test]
fn sql_prune_preserves_rows_matching_rust_predicate() {
    let dir = TempDir::new().expect("tempdir");
    let store = open_store(&dir.path().join("state.sqlite"));
    let old = (Utc::now() - Duration::days(40)).to_rfc3339();

    store
        .add_audit_event_at("daemon_cycle_summary", &json!({"ok": true}), None, &old)
        .expect("prunable noise");

    let preserved_specs: Vec<(&str, serde_json::Value)> = vec![
        (TAKER_DETECTION, json!({"offer_id": "offer1"})),
        (
            COIN_OP_LEDGER_EXECUTED,
            json!({"operation_id": "coin-op-1"}),
        ),
        (COIN_OPS_EXECUTED, json!({"batch_id": "batch-1"})),
        (
            OFFER_LIFECYCLE_TRANSITION,
            json!({"new_state":"cancelled","reason":"cancel_tx_chain_confirmed"}),
        ),
        (
            OFFER_LIFECYCLE_TRANSITION,
            json!({"new_state":"cancelled","reason":"ok"}),
        ),
        (
            OFFER_LIFECYCLE_TRANSITION,
            json!({"new_state":"mempool_observed","reason":"potential_take_seen"}),
        ),
        (
            OFFER_LIFECYCLE_TRANSITION,
            json!({"new_state":"mempool_observed","reason":"coinset_mempool_observed"}),
        ),
        (
            OFFER_LIFECYCLE_TRANSITION,
            json!({"new_state":"tx_block_confirmed","reason":"take_confirmed_on_tx_block"}),
        ),
        (
            OFFER_LIFECYCLE_TRANSITION,
            json!({"new_state":"tx_block_confirmed","reason":"coinset_tx_block_webhook_confirmed"}),
        ),
        (
            OFFER_CANCEL_POLICY,
            json!({"executed_count": 1, "items": []}),
        ),
    ];

    for (event_type, payload) in &preserved_specs {
        assert!(
            is_preserved_audit_row(event_type, payload),
            "seed row must match Rust preserve predicate: {event_type}"
        );
        store
            .add_audit_event_at(event_type, payload, Some("m1"), &old)
            .expect("preserved row");
    }

    let report = store
        .prune_stale_audit_events(
            DEFAULT_AUDIT_RETENTION_DAYS,
            PruneAuditEventsOptions::cli(false, false),
        )
        .expect("prune");
    assert_eq!(report.deleted_count, 1);

    let remaining = store
        .list_recent_audit_events(None, None, 100)
        .expect("list");
    assert_eq!(remaining.len(), preserved_specs.len());
    for row in remaining {
        assert!(
            is_preserved_audit_row(&row.event_type, &row.payload),
            "SQL delete removed preserved row: {}",
            row.event_type
        );
    }
}
