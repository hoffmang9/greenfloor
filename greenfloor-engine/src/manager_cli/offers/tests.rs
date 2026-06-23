#![allow(clippy::large_futures)]

use std::collections::HashMap;
use std::path::Path;

use clap::Parser;
use serde_json::json;

use crate::manager_cli::commands::ManagerCli;
use crate::manager_cli::test_support::{
    pop_json, write_combine_dust_markets, ManagerContextBuilder,
};
use crate::minimal_program_template::{
    write_minimal_program, write_minimal_program_with_signer, MinimalProgramParams,
};
use crate::storage::SqliteStore;

use super::{
    run_offers_cancel_command, run_offers_reconcile_command, OffersCancelCliArgs,
    OffersReconcileCliArgs,
};

const TEST_RECEIVE_ADDRESS: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";
const TEST_CAT_ASSET_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

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

fn write_cancel_test_markets(path: &Path) {
    write_combine_dust_markets(path, TEST_CAT_ASSET_ID, TEST_RECEIVE_ADDRESS);
    let mut yaml = std::fs::read_to_string(path).expect("read markets");
    yaml = yaml.replace("dust_m", "m1");
    std::fs::write(path, yaml).expect("write markets");
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
async fn offers_cancel_cancel_open_selects_open_offers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_cancel_test_markets(&markets);
    let db_path = dir.path().join("db").join("greenfloor.sqlite");
    seed_offer_states(
        &db_path,
        &[
            ("offer-open", "m1", "open"),
            ("offer-expired", "m1", "expired"),
        ],
    );

    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/v1/offers/offer-open")
        .with_status(200)
        .with_body(json!({"id":"offer-open","status":0}).to_string())
        .create_async()
        .await;
    write_minimal_program_with_signer(
        &program,
        MinimalProgramParams {
            home_dir: dir.path(),
            dexie_api_base: &server.url(),
            low_inventory_alerts_enabled: true,
            pushover_enabled: true,
            ..Default::default()
        },
    );
    let harness = ManagerContextBuilder::new(program, markets)
        .scratch_dir(dir.path().to_path_buf())
        .build_capturing();
    let code = run_offers_cancel_command(
        &harness.ctx,
        OffersCancelCliArgs {
            offer_id: vec![],
            offer_file: vec![],
            market_id: None,
            cancel_open: true,
            venue: None,
        },
    )
    .await
    .expect("offers-cancel");
    assert_eq!(code, 2);
    let payload = pop_json(&harness.captured);
    assert_eq!(payload.get("venue"), Some(&json!("dexie")));
    assert_eq!(payload.get("selected_count"), Some(&json!(1)));
    assert_eq!(payload.get("submitted_count"), Some(&json!(0)));
    assert_eq!(payload.get("failed_count"), Some(&json!(1)));
}

#[tokio::test]
async fn offers_cancel_by_offer_id_fetches_dexie_offer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_cancel_test_markets(&markets);
    let db_path = dir.path().join("db").join("greenfloor.sqlite");
    seed_offer_states(
        &db_path,
        &[
            ("offer-target", "m1", "open"),
            ("offer-other", "m1", "open"),
        ],
    );

    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/v1/offers/offer-target")
        .with_status(200)
        .with_body(json!({"id":"offer-target","status":0}).to_string())
        .create_async()
        .await;
    write_minimal_program_with_signer(
        &program,
        MinimalProgramParams {
            home_dir: dir.path(),
            dexie_api_base: &server.url(),
            ..Default::default()
        },
    );
    let harness = ManagerContextBuilder::new(program, markets)
        .scratch_dir(dir.path().to_path_buf())
        .build_capturing();
    let code = run_offers_cancel_command(
        &harness.ctx,
        OffersCancelCliArgs {
            offer_id: vec!["offer-target".to_string()],
            offer_file: vec![],
            market_id: None,
            cancel_open: false,
            venue: None,
        },
    )
    .await
    .expect("offers-cancel");
    assert_eq!(code, 2);
    let payload = pop_json(&harness.captured);
    assert_eq!(payload.get("selected_count"), Some(&json!(1)));
    assert_eq!(payload.get("failed_count"), Some(&json!(1)));
}

#[tokio::test]
async fn offers_cancel_reports_dexie_failure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let program = dir.path().join("program.yaml");
    let markets = dir.path().join("markets.yaml");
    write_cancel_test_markets(&markets);
    let db_path = dir.path().join("db").join("greenfloor.sqlite");
    seed_offer_states(&db_path, &[("offer-fail", "m1", "open")]);

    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/v1/offers/offer-fail")
        .with_status(404)
        .with_body(r#"{"success":false,"error":"not_found"}"#)
        .create_async()
        .await;
    write_minimal_program_with_signer(
        &program,
        MinimalProgramParams {
            home_dir: dir.path(),
            dexie_api_base: &server.url(),
            ..Default::default()
        },
    );
    let harness = ManagerContextBuilder::new(program, markets)
        .scratch_dir(dir.path().to_path_buf())
        .build_capturing();
    let code = run_offers_cancel_command(
        &harness.ctx,
        OffersCancelCliArgs {
            offer_id: vec!["offer-fail".to_string()],
            offer_file: vec![],
            market_id: None,
            cancel_open: false,
            venue: None,
        },
    )
    .await
    .expect("offers-cancel");
    assert_eq!(code, 2);
    let payload = pop_json(&harness.captured);
    assert_eq!(payload.get("submitted_count"), Some(&json!(0)));
    assert_eq!(payload.get("failed_count"), Some(&json!(1)));
    let item = payload
        .get("items")
        .and_then(|value| value.as_array())
        .and_then(|items| items.first())
        .expect("item");
    let error = item
        .get("result")
        .and_then(|value| value.get("error"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    assert!(error.contains("offer_cancel_dexie_offer_not_found"));
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
