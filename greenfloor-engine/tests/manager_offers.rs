#[path = "fixtures/json_util.rs"]
mod json_util;
#[path = "fixtures/manager.rs"]
mod manager_fixtures;

use std::path::Path;

use greenfloor_engine::storage::SqliteStore;
use manager_fixtures::{
    parse_json_output, patch_program_dexie_base, restore_program_dexie_base, run_manager,
    write_manager_program,
};
use serde_json::json;

fn seed_offer_states(db_path: &Path, rows: &[(&str, &str, &str)]) {
    let store = SqliteStore::open(db_path).expect("open db");
    for (offer_id, market_id, state) in rows {
        store
            .upsert_offer_state(offer_id, market_id, state, Some(0))
            .expect("seed offer");
    }
}

#[tokio::test]
async fn offers_reconcile_updates_states_from_dexie() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let db_path = dir.path().join("state.sqlite");
    write_manager_program(&program, dir.path());
    let confirmed_tx_id = "a".repeat(64);
    {
        let store = SqliteStore::open(&db_path).expect("open db");
        store
            .upsert_offer_state("offer-ok", "m1", "open", Some(0))
            .expect("seed");
        store
            .upsert_offer_state("offer-missing", "m1", "open", Some(0))
            .expect("seed");
        store
            .observe_mempool_tx_ids(std::slice::from_ref(&confirmed_tx_id))
            .expect("observe");
        store.confirm_tx_ids(&[confirmed_tx_id]).expect("confirm");
    }

    let mut server = mockito::Server::new_async().await;
    let confirmed_tx_id = "a".repeat(64);
    let _ok = server
        .mock("GET", "/v1/offers/offer-ok")
        .with_status(200)
        .with_body(json!({"id":"offer-ok","status":4,"tx_id": confirmed_tx_id}).to_string())
        .create_async()
        .await;
    let _missing = server
        .mock("GET", "/v1/offers/offer-missing")
        .with_status(404)
        .with_body(r#"{"success":false,"error":"not_found"}"#)
        .create_async()
        .await;
    let original = std::fs::read_to_string(&program).expect("read program");
    patch_program_dexie_base(&program, &server.url());
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "--state-db",
            db_path.to_str().expect("db"),
            "offers-reconcile",
            "--limit",
            "20",
            "--venue",
            "dexie",
        ],
        None,
        None,
    );
    restore_program_dexie_base(&program, &original);
    assert_eq!(output.status.code(), Some(0));
    let payload = parse_json_output(&output.stdout);
    assert_eq!(payload.get("reconciled_count"), Some(&json!(2)));
    assert_eq!(payload.get("changed_count"), Some(&json!(2)));

    let store = SqliteStore::open(&db_path).expect("open db");
    let rows = store.list_offer_states(None, 20).expect("rows");
    let by_id = rows
        .iter()
        .map(|row| (row.offer_id.as_str(), row))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(
        by_id.get("offer-ok").expect("offer-ok").state,
        "tx_block_confirmed"
    );
    assert_eq!(
        by_id.get("offer-missing").expect("missing").state,
        "expired"
    );
}

#[tokio::test]
async fn offers_cancel_cancel_open_uses_dexie() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let db_path = dir.path().join("db").join("greenfloor.sqlite");
    write_manager_program(&program, dir.path());
    seed_offer_states(
        &db_path,
        &[
            ("offer-open", "m1", "open"),
            ("offer-expired", "m1", "expired"),
        ],
    );

    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/v1/offers/offer-open/cancel")
        .with_status(200)
        .with_body(r#"{"success":true,"id":"offer-open","status":3}"#)
        .create_async()
        .await;
    let original = std::fs::read_to_string(&program).expect("read program");
    patch_program_dexie_base(&program, &server.url());
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "offers-cancel",
            "--cancel-open",
        ],
        None,
        None,
    );
    restore_program_dexie_base(&program, &original);
    assert_eq!(output.status.code(), Some(0));
    let payload = parse_json_output(&output.stdout);
    assert_eq!(payload.get("venue"), Some(&json!("dexie")));
    assert_eq!(payload.get("selected_count"), Some(&json!(1)));
    assert_eq!(payload.get("cancelled_count"), Some(&json!(1)));
    assert_eq!(payload.get("failed_count"), Some(&json!(0)));

    let store = SqliteStore::open(&db_path).expect("open db");
    let rows = store.list_offer_states(None, 10).expect("rows");
    let by_id = rows
        .iter()
        .map(|row| (row.offer_id.as_str(), row))
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(by_id.get("offer-open").expect("open").state, "cancelled");
    assert_eq!(
        by_id.get("offer-open").expect("open").last_seen_status,
        Some(3)
    );
    assert_eq!(
        by_id.get("offer-expired").expect("expired").state,
        "expired"
    );
}

#[tokio::test]
async fn offers_cancel_by_offer_id_uses_dexie() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let db_path = dir.path().join("db").join("greenfloor.sqlite");
    write_manager_program(&program, dir.path());
    seed_offer_states(
        &db_path,
        &[
            ("offer-target", "m1", "open"),
            ("offer-other", "m1", "open"),
        ],
    );

    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/v1/offers/offer-target/cancel")
        .with_status(200)
        .with_body(r#"{"success":true,"id":"offer-target","status":3}"#)
        .create_async()
        .await;
    let original = std::fs::read_to_string(&program).expect("read program");
    patch_program_dexie_base(&program, &server.url());
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "offers-cancel",
            "--offer-id",
            "offer-target",
        ],
        None,
        None,
    );
    restore_program_dexie_base(&program, &original);
    assert_eq!(output.status.code(), Some(0));
    let payload = parse_json_output(&output.stdout);
    assert_eq!(payload.get("selected_count"), Some(&json!(1)));
    assert_eq!(payload.get("cancelled_count"), Some(&json!(1)));
}

#[tokio::test]
async fn offers_cancel_reports_dexie_failure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let db_path = dir.path().join("db").join("greenfloor.sqlite");
    write_manager_program(&program, dir.path());
    seed_offer_states(&db_path, &[("offer-fail", "m1", "open")]);

    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/v1/offers/offer-fail/cancel")
        .with_status(200)
        .with_body(r#"{"success":false,"error":"not_found"}"#)
        .create_async()
        .await;
    let original = std::fs::read_to_string(&program).expect("read program");
    patch_program_dexie_base(&program, &server.url());
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "offers-cancel",
            "--offer-id",
            "offer-fail",
        ],
        None,
        None,
    );
    restore_program_dexie_base(&program, &original);
    assert_eq!(output.status.code(), Some(2));
    let payload = parse_json_output(&output.stdout);
    assert_eq!(payload.get("cancelled_count"), Some(&json!(0)));
    assert_eq!(payload.get("failed_count"), Some(&json!(1)));
}

#[test]
fn offers_cancel_rejects_removed_submit_onchain_flag() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    write_manager_program(&program, dir.path());
    let output = run_manager(
        &[
            "--program-config",
            program.to_str().expect("program"),
            "offers-cancel",
            "--offer-id",
            "offer-1",
            "--submit-onchain-after-offchain",
        ],
        None,
        None,
    );
    assert_ne!(output.status.code(), Some(0));
}
