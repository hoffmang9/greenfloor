use std::collections::HashMap;
use std::path::Path;

use clap::Parser;
use serde_json::json;

use crate::manager_cli::commands::ManagerCli;
use crate::manager_cli::test_support::{pop_json, ManagerContextBuilder};
use crate::minimal_program_template::{write_minimal_program, MinimalProgramParams};
use crate::storage::SqliteStore;

use super::{
    run_offers_cancel_command, run_offers_reconcile_command, OffersCancelCliArgs,
    OffersReconcileCliArgs,
};

fn write_offers_program(path: &Path, home_dir: &Path, dexie_api_base: &str) {
    write_minimal_program(
        path,
        MinimalProgramParams {
            home_dir,
            dexie_api_base,
            low_inventory_alerts_enabled: true,
            pushover_enabled: true,
            ..Default::default()
        },
    );
}

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
    let markets = dir.path().join("markets.yaml");
    std::fs::write(&markets, "markets: []\n").expect("write markets");
    let db_path = dir.path().join("state.sqlite");
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
    write_offers_program(&program, dir.path(), &server.url());
    let harness = ManagerContextBuilder::new(program, markets)
        .scratch_dir(dir.path().to_path_buf())
        .state_db(db_path.to_str().expect("db path"))
        .build_capturing();
    let code = run_offers_reconcile_command(
        &harness.ctx,
        OffersReconcileCliArgs {
            market_id: String::new(),
            limit: 20,
            venue: Some("dexie".to_string()),
        },
    )
    .await
    .expect("offers-reconcile");
    assert_eq!(code, 0);
    let payload = pop_json(&harness.captured);
    assert_eq!(payload.get("reconciled_count"), Some(&json!(2)));
    assert_eq!(payload.get("changed_count"), Some(&json!(2)));

    let store = SqliteStore::open(&db_path).expect("open db");
    let rows = store.list_offer_states(None, 20).expect("rows");
    let by_id: HashMap<_, _> = rows
        .iter()
        .map(|row| (row.offer_id.as_str(), row))
        .collect();
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
    let markets = dir.path().join("markets.yaml");
    std::fs::write(&markets, "markets: []\n").expect("write markets");
    let db_path = dir.path().join("db").join("greenfloor.sqlite");
    write_offers_program(&program, dir.path(), "https://api.dexie.space");
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
    write_offers_program(&program, dir.path(), &server.url());
    let harness = ManagerContextBuilder::new(program, markets)
        .scratch_dir(dir.path().to_path_buf())
        .build_capturing();
    let code = run_offers_cancel_command(
        &harness.ctx,
        OffersCancelCliArgs {
            offer_id: vec![],
            cancel_open: true,
            venue: None,
        },
    )
    .await
    .expect("offers-cancel");
    assert_eq!(code, 0);
    let payload = pop_json(&harness.captured);
    assert_eq!(payload.get("venue"), Some(&json!("dexie")));
    assert_eq!(payload.get("selected_count"), Some(&json!(1)));
    assert_eq!(payload.get("cancelled_count"), Some(&json!(1)));
    assert_eq!(payload.get("failed_count"), Some(&json!(0)));

    let store = SqliteStore::open(&db_path).expect("open db");
    let rows = store.list_offer_states(None, 10).expect("rows");
    let by_id: HashMap<_, _> = rows
        .iter()
        .map(|row| (row.offer_id.as_str(), row))
        .collect();
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
    let markets = dir.path().join("markets.yaml");
    std::fs::write(&markets, "markets: []\n").expect("write markets");
    let db_path = dir.path().join("db").join("greenfloor.sqlite");
    write_offers_program(&program, dir.path(), "https://api.dexie.space");
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
    write_offers_program(&program, dir.path(), &server.url());
    let harness = ManagerContextBuilder::new(program, markets)
        .scratch_dir(dir.path().to_path_buf())
        .build_capturing();
    let code = run_offers_cancel_command(
        &harness.ctx,
        OffersCancelCliArgs {
            offer_id: vec!["offer-target".to_string()],
            cancel_open: false,
            venue: None,
        },
    )
    .await
    .expect("offers-cancel");
    assert_eq!(code, 0);
    let payload = pop_json(&harness.captured);
    assert_eq!(payload.get("selected_count"), Some(&json!(1)));
    assert_eq!(payload.get("cancelled_count"), Some(&json!(1)));
}

#[tokio::test]
async fn offers_cancel_reports_dexie_failure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    std::fs::write(&markets, "markets: []\n").expect("write markets");
    let db_path = dir.path().join("db").join("greenfloor.sqlite");
    write_offers_program(&program, dir.path(), "https://api.dexie.space");
    seed_offer_states(&db_path, &[("offer-fail", "m1", "open")]);

    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/v1/offers/offer-fail/cancel")
        .with_status(200)
        .with_body(r#"{"success":false,"error":"not_found"}"#)
        .create_async()
        .await;
    write_offers_program(&program, dir.path(), &server.url());
    let harness = ManagerContextBuilder::new(program, markets)
        .scratch_dir(dir.path().to_path_buf())
        .build_capturing();
    let code = run_offers_cancel_command(
        &harness.ctx,
        OffersCancelCliArgs {
            offer_id: vec!["offer-fail".to_string()],
            cancel_open: false,
            venue: None,
        },
    )
    .await
    .expect("offers-cancel");
    assert_eq!(code, 2);
    let payload = pop_json(&harness.captured);
    assert_eq!(payload.get("cancelled_count"), Some(&json!(0)));
    assert_eq!(payload.get("failed_count"), Some(&json!(1)));
}

#[test]
fn offers_cancel_rejects_removed_submit_onchain_flag() {
    assert!(ManagerCli::try_parse_from([
        "greenfloor-manager",
        "--program-config",
        "/tmp/program.yaml",
        "offers-cancel",
        "--offer-id",
        "offer-1",
        "--submit-onchain-after-offchain",
    ])
    .is_err());
}
