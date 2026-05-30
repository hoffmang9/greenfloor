use std::collections::BTreeMap;
use std::path::PathBuf;

use tempfile::tempdir;

use crate::config::ManagerProgramConfig;
use crate::cycle::parallel_managed_dispatch_enabled;
use crate::error::SignerError;
use crate::storage::SqliteStore;
use super::{
    is_parallel_dispatch_transient_signer_error, record_parallel_fallback_audit,
    OfferReservationCoordinator,
};

fn sample_program(parallelism_enabled: bool, dry_run: bool) -> ManagerProgramConfig {
    ManagerProgramConfig {
        network: "mainnet".to_string(),
        home_dir: PathBuf::from("/tmp/gf"),
        app_log_level: "INFO".to_string(),
        app_log_level_was_missing: false,
        dexie_api_base: "https://api.dexie.space".to_string(),
        splash_api_base: "http://localhost:4000".to_string(),
        offer_publish_venue: "dexie".to_string(),
        coin_ops_minimum_fee_mojos: 0,
        coin_ops_max_operations_per_run: 0,
        coin_ops_max_daily_fee_budget_mojos: 0,
        coin_ops_split_fee_mojos: 0,
        coin_ops_combine_fee_mojos: 0,
        runtime_offer_bootstrap_wait_timeout_seconds: 120,
        runtime_market_slot_count: 1,
        runtime_parallel_markets: false,
        runtime_offer_parallelism_enabled: parallelism_enabled,
        runtime_offer_parallelism_max_workers: 2,
        runtime_dry_run: dry_run,
        runtime_loop_interval_seconds: 30,
        tx_block_trigger_mode: "websocket".to_string(),
        tx_block_websocket_url: String::new(),
        tx_block_websocket_reconnect_interval_seconds: 1,
        tx_block_fallback_poll_interval_seconds: 1,
    }
}

#[test]
fn parallel_managed_dispatch_enabled_requires_parallelism_and_live_runtime() {
    let mut program = sample_program(true, false);
    assert!(parallel_managed_dispatch_enabled(&program));
    program.runtime_offer_parallelism_enabled = false;
    assert!(!parallel_managed_dispatch_enabled(&program));
    program.runtime_offer_parallelism_enabled = true;
    program.runtime_dry_run = true;
    assert!(!parallel_managed_dispatch_enabled(&program));
}

#[test]
fn parallel_transient_signer_error_classifies_reservation_and_upstream() {
    let contention = SignerError::Other("ReservationContentionError: busy".to_string());
    assert!(is_parallel_dispatch_transient_signer_error(&contention));
    let upstream = SignerError::Other("ManagedUpstreamTransientError: timeout".to_string());
    assert!(is_parallel_dispatch_transient_signer_error(&upstream));
    let locked = SignerError::Other("database is locked".to_string());
    assert!(is_parallel_dispatch_transient_signer_error(&locked));
    let fatal = SignerError::Other("permanent_offer_build_failure: bad puzzle".to_string());
    assert!(!is_parallel_dispatch_transient_signer_error(&fatal));
}

#[test]
fn coordinator_second_acquire_fails_when_capacity_exhausted() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let coordinator = OfferReservationCoordinator::new(&db_path, Some(300));
    let market_id = "m1";
    let wallet_id = "wallet-1";
    let mut requested = BTreeMap::from([("asset-a".to_string(), 100_i64)]);
    let mut available = BTreeMap::from([("asset-a".to_string(), 100_i64)]);

    let first = coordinator
        .try_acquire(market_id, wallet_id, &requested, &available)
        .expect("first acquire");
    assert!(first.ok);
    assert!(first.reservation_id.is_some());

    let second = coordinator
        .try_acquire(market_id, wallet_id, &requested, &available)
        .expect("second acquire");
    assert!(!second.ok);
    let error = second.error.expect("contention error");
    assert!(error.contains("reservation_insufficient"));
}

#[test]
fn coordinator_release_frees_capacity_for_next_acquire() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let coordinator = OfferReservationCoordinator::new(&db_path, Some(300));
    let market_id = "m1";
    let wallet_id = "wallet-1";
    let requested = BTreeMap::from([("asset-a".to_string(), 50_i64)]);
    let available = BTreeMap::from([("asset-a".to_string(), 50_i64)]);

    let acquired = coordinator
        .try_acquire(market_id, wallet_id, &requested, &available)
        .expect("acquire");
    let reservation_id = acquired.reservation_id.expect("reservation id");
    coordinator
        .release(&reservation_id, "released_success")
        .expect("release");

    let after_release = coordinator
        .try_acquire(market_id, wallet_id, &requested, &available)
        .expect("acquire after release");
    assert!(after_release.ok);
}

#[tokio::test]
async fn record_parallel_fallback_audit_persists_event() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let store = SqliteStore::open(&db_path).expect("open");
    let err = SignerError::Other("ReservationContentionError: simulated".to_string());
    record_parallel_fallback_audit(&store, "m1", &err)
        .await
        .expect("audit");
    let events = store
        .list_recent_audit_events(
            Some(&["offer_parallel_fallback"]),
            Some("m1"),
            5,
        )
        .expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "offer_parallel_fallback");
}

#[tokio::test]
async fn execute_strategy_actions_skips_when_signer_path_missing() {
    use super::execute_strategy_actions;
    use crate::config::MarketConfig;
    use crate::cycle::PlannedAction;
    use serde_json::json;
    use std::collections::HashMap;

    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let store = SqliteStore::open(&db_path).expect("open");
    let program_path = dir.path().join("program.yaml");
    std::fs::write(&program_path, "app:\n  network: mainnet\n").expect("write program");
    let markets_path = dir.path().join("markets.yaml");
    std::fs::write(&markets_path, "markets: []\n").expect("write markets");
    let program = sample_program(false, false);
    let market = MarketConfig {
        market_id: "m1".to_string(),
        enabled: true,
        base_asset: "xch".to_string(),
        base_symbol: "XCH".to_string(),
        quote_asset: "xch".to_string(),
        quote_asset_type: "stable".to_string(),
        receive_address: "xch1test".to_string(),
        signer_key_id: "key-1".to_string(),
        mode: "sell_only".to_string(),
        pricing: json!({}),
        cancel_move_threshold_bps: None,
        ladders: HashMap::new(),
    };
    let actions = vec![PlannedAction {
        size: 1,
        repeat: 1,
        pair: "xch".to_string(),
        expiry_unit: "minutes".to_string(),
        expiry_value: 10,
        cancel_after_create: false,
        reason: "test".to_string(),
        target_spread_bps: None,
        side: "sell".to_string(),
    }];

    let output = execute_strategy_actions(
        &store,
        &db_path,
        &program,
        &market,
        "mainnet",
        &program_path,
        &markets_path,
        None,
        &actions,
    )
    .await
    .expect("dispatch");

    assert_eq!(output.executed_count, 0);
    let events = store
        .list_recent_audit_events(Some(&["strategy_exec_skipped_no_signer"]), Some("m1"), 1)
        .expect("events");
    assert_eq!(events.len(), 1);
}
