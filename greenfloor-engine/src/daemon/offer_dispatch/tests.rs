use std::collections::BTreeMap;

use tempfile::tempdir;

use super::{
    classify_parallel_dispatch, coordinator::OfferReservationCoordinator,
    is_parallel_dispatch_transient_signer_error, record_parallel_fallback_audit,
    OfferDispatchOutput, ParallelDispatchDecision,
};
use crate::config::{load_program_bundle, ManagerProgramConfig, MarketConfig};
use crate::cycle::{parallel_managed_dispatch_enabled, PlannedAction};
use crate::daemon::test_support::test_cycle_context;
use crate::error::SignerError;
use crate::storage::SqliteStore;
use crate::test_support::minimal_program::{
    write_minimal_program_with_signer, MinimalProgramParams,
};
use serde_json::json;
use std::collections::HashMap;

fn write_test_markets_file(path: &std::path::Path) {
    std::fs::write(
        path,
        r"
markets:
  - id: m1
    enabled: true
    base_asset: asset1
    base_symbol: AS1
    quote_asset: xch
    quote_asset_type: unstable
    receive_address: xch1test
    signer_key_id: key-1
    mode: sell_only
    pricing: {}
",
    )
    .expect("write markets");
}

fn test_context_from_program_file(
    dir: &tempfile::TempDir,
    db_path: &std::path::Path,
    program_path: &std::path::Path,
    mut program: ManagerProgramConfig,
    with_signer: bool,
) -> crate::daemon::test_support::TestCycleContextBundle {
    let signer = if with_signer {
        let bundle = load_program_bundle(program_path).expect("program bundle");
        program.signer_kms_key_id = bundle.program.signer_kms_key_id;
        program.vault_launcher_id = bundle.program.vault_launcher_id;
        Some(bundle.signer)
    } else {
        None
    };
    test_cycle_context(dir, db_path, program, signer)
}

fn sample_program(parallelism_enabled: bool, dry_run: bool) -> ManagerProgramConfig {
    ManagerProgramConfig {
        runtime_market_slot_count: 1,
        runtime_offer_parallelism_enabled: parallelism_enabled,
        runtime_offer_parallelism_max_workers: 2,
        runtime_dry_run: dry_run,
        tx_block_websocket_reconnect_interval_seconds: 1,
        tx_block_fallback_poll_interval_seconds: 1,
        ..Default::default()
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
    let contention = SignerError::ReservationContention("busy".to_string());
    assert!(is_parallel_dispatch_transient_signer_error(&contention));
    let upstream = SignerError::ManagedUpstreamTransient("timeout".to_string());
    assert!(is_parallel_dispatch_transient_signer_error(&upstream));
    let locked = SignerError::DatabaseLocked;
    assert!(is_parallel_dispatch_transient_signer_error(&locked));
    let fatal = SignerError::Other("permanent_offer_build_failure: bad puzzle".to_string());
    assert!(!is_parallel_dispatch_transient_signer_error(&fatal));
}

#[test]
fn classify_parallel_dispatch_success_returns_output() {
    let output = OfferDispatchOutput {
        executed_count: 2,
        newly_executed_sell_counts: BTreeMap::from([(1, 2)]),
    };
    match classify_parallel_dispatch(Ok(output.clone())) {
        ParallelDispatchDecision::Success(value) => assert_eq!(value.executed_count, 2),
        _ => panic!("expected success"),
    }
}

#[test]
fn classify_parallel_dispatch_transient_error_falls_back() {
    let err = SignerError::Other("ReservationContentionError: busy".to_string());
    match classify_parallel_dispatch(Err(err)) {
        ParallelDispatchDecision::FallbackTransient(message) => {
            assert!(message.to_string().contains("ReservationContentionError"));
        }
        _ => panic!("expected transient fallback"),
    }
}

#[test]
fn classify_parallel_dispatch_fatal_error_propagates() {
    let err = SignerError::Other("permanent_offer_build_failure: bad puzzle".to_string());
    match classify_parallel_dispatch(Err(err)) {
        ParallelDispatchDecision::Fatal(message) => {
            assert!(message
                .to_string()
                .contains("permanent_offer_build_failure"));
        }
        _ => panic!("expected fatal"),
    }
}

#[test]
fn coordinator_concurrent_acquires_only_one_succeeds_for_full_capacity() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let coordinator = OfferReservationCoordinator::new(&db_path, Some(300)).expect("coordinator");
    let market_id = "m1";
    let wallet_id = "wallet-1";
    let requested = BTreeMap::from([("asset-a".to_string(), 100_i64)]);
    let available = BTreeMap::from([("asset-a".to_string(), 100_i64)]);

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
    let coordinator = OfferReservationCoordinator::new(&db_path, Some(300)).expect("coordinator");
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
    record_parallel_fallback_audit(&store, "m1", &err).expect("audit");
    let events = store
        .list_recent_audit_events(Some(&["offer_parallel_fallback"]), Some("m1"), 5)
        .expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "offer_parallel_fallback");
}

#[tokio::test]
async fn execute_strategy_actions_parallel_disabled_uses_sequential_skip_path() {
    use super::execute_strategy_actions;

    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let store = SqliteStore::open(&db_path).expect("open");
    let program_path = dir.path().join("program.yaml");
    write_minimal_program_with_signer(
        &program_path,
        MinimalProgramParams {
            home_dir: dir.path(),
            ..Default::default()
        },
    );
    let markets_path = dir.path().join("markets.yaml");
    write_test_markets_file(&markets_path);
    let test_ctx = test_context_from_program_file(
        &dir,
        &db_path,
        &program_path,
        sample_program(false, false),
        false,
    );
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
        ladders: HashMap::default(),
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

    let output = execute_strategy_actions(&store, &test_ctx.cycle_context(), &market, &actions)
        .await
        .expect("dispatch");

    assert_eq!(output.executed_count, 0);
    let events = store
        .list_recent_audit_events(Some(&["strategy_exec_skipped_no_signer"]), Some("m1"), 1)
        .expect("events");
    assert_eq!(events.len(), 1);
}

fn sample_market() -> MarketConfig {
    MarketConfig {
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
        ladders: HashMap::default(),
    }
}

fn sample_action() -> PlannedAction {
    PlannedAction {
        size: 1,
        repeat: 1,
        pair: "xch".to_string(),
        expiry_unit: "minutes".to_string(),
        expiry_value: 10,
        cancel_after_create: false,
        reason: "test".to_string(),
        target_spread_bps: None,
        side: "sell".to_string(),
    }
}

#[tokio::test]
async fn execute_strategy_actions_parallel_transient_falls_back_to_sequential() {
    use super::execute_strategy_actions;
    use crate::daemon::run_once::{
        ManagedPostTestMode, OfferDispatchTestOverrides, ParallelDispatchTestMode,
    };

    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let store = SqliteStore::open(&db_path).expect("open");
    let program_path = dir.path().join("program.yaml");
    write_minimal_program_with_signer(
        &program_path,
        MinimalProgramParams {
            home_dir: dir.path(),
            ..Default::default()
        },
    );
    let markets_path = dir.path().join("markets.yaml");
    write_test_markets_file(&markets_path);
    let mut test_ctx = test_context_from_program_file(
        &dir,
        &db_path,
        &program_path,
        sample_program(true, false),
        true,
    );
    test_ctx.dispatch.test_controls.offer_dispatch = OfferDispatchTestOverrides::default()
        .parallel_dispatch(ParallelDispatchTestMode::Transient)
        .managed_post(ManagedPostTestMode::Success);

    let output = execute_strategy_actions(
        &store,
        &test_ctx.cycle_context(),
        &sample_market(),
        &[sample_action()],
    )
    .await
    .expect("dispatch");
    assert_eq!(output.executed_count, 1);
}

#[tokio::test]
async fn execute_strategy_actions_parallel_fatal_propagates() {
    use super::execute_strategy_actions;
    use crate::daemon::run_once::{OfferDispatchTestOverrides, ParallelDispatchTestMode};

    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let store = SqliteStore::open(&db_path).expect("open");
    let program_path = dir.path().join("program.yaml");
    write_minimal_program_with_signer(
        &program_path,
        MinimalProgramParams {
            home_dir: dir.path(),
            ..Default::default()
        },
    );
    let markets_path = dir.path().join("markets.yaml");
    write_test_markets_file(&markets_path);
    let mut test_ctx = test_context_from_program_file(
        &dir,
        &db_path,
        &program_path,
        sample_program(true, false),
        true,
    );
    test_ctx.dispatch.test_controls.offer_dispatch =
        OfferDispatchTestOverrides::default().parallel_dispatch(ParallelDispatchTestMode::Fatal);

    let err = execute_strategy_actions(
        &store,
        &test_ctx.cycle_context(),
        &sample_market(),
        &[sample_action()],
    )
    .await
    .expect_err("fatal parallel error");
    assert!(err.to_string().contains("permanent_offer_build_failure"));
}

#[tokio::test]
async fn execute_strategy_actions_managed_post_success_via_sequential_path() {
    use super::execute_strategy_actions;
    use crate::daemon::run_once::{ManagedPostTestMode, OfferDispatchTestOverrides};

    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let store = SqliteStore::open(&db_path).expect("open");
    let program_path = dir.path().join("program.yaml");
    write_minimal_program_with_signer(
        &program_path,
        MinimalProgramParams {
            home_dir: dir.path(),
            ..Default::default()
        },
    );
    let markets_path = dir.path().join("markets.yaml");
    write_test_markets_file(&markets_path);
    let mut test_ctx = test_context_from_program_file(
        &dir,
        &db_path,
        &program_path,
        sample_program(false, false),
        true,
    );
    test_ctx.dispatch.test_controls.offer_dispatch =
        OfferDispatchTestOverrides::default().managed_post(ManagedPostTestMode::Success);

    let output = execute_strategy_actions(
        &store,
        &test_ctx.cycle_context(),
        &sample_market(),
        &[sample_action()],
    )
    .await
    .expect("dispatch");
    assert_eq!(output.executed_count, 1);
}

#[test]
fn coordinator_partial_acquire_rejects_when_requested_exceeds_available() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let coordinator = OfferReservationCoordinator::new(&db_path, Some(300)).expect("coordinator");
    let requested = BTreeMap::from([("asset-a".to_string(), 80_i64)]);
    let available = BTreeMap::from([("asset-a".to_string(), 50_i64)]);

    let result = coordinator
        .try_acquire("m1", "wallet-1", &requested, &available)
        .expect("acquire");
    assert!(!result.ok);
    assert!(result
        .error
        .expect("error")
        .contains("reservation_insufficient"));
}

#[test]
fn coordinator_second_acquire_blocked_until_release() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let coordinator = OfferReservationCoordinator::new(&db_path, Some(300)).expect("coordinator");
    let requested = BTreeMap::from([("asset-a".to_string(), 40_i64)]);
    let available = BTreeMap::from([("asset-a".to_string(), 40_i64)]);

    let first = coordinator
        .try_acquire("m1", "wallet-1", &requested, &available)
        .expect("first acquire");
    assert!(first.ok);
    let reservation_id = first.reservation_id.expect("reservation id");

    let blocked = coordinator
        .try_acquire("m1", "wallet-1", &requested, &available)
        .expect("blocked acquire");
    assert!(!blocked.ok);

    coordinator
        .release(&reservation_id, "released_success")
        .expect("release");

    let after_release = coordinator
        .try_acquire("m1", "wallet-1", &requested, &available)
        .expect("acquire after release");
    assert!(after_release.ok);
}

#[test]
fn coordinator_multi_asset_acquire_requires_all_assets() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let coordinator = OfferReservationCoordinator::new(&db_path, Some(300)).expect("coordinator");
    let requested = BTreeMap::from([
        ("asset-a".to_string(), 10_i64),
        ("asset-b".to_string(), 10_i64),
    ]);
    let available = BTreeMap::from([
        ("asset-a".to_string(), 10_i64),
        ("asset-b".to_string(), 5_i64),
    ]);

    let result = coordinator
        .try_acquire("m1", "wallet-1", &requested, &available)
        .expect("acquire");
    assert!(!result.ok);
}

#[test]
fn offer_dispatch_parallel_override_success_returns_output() {
    use super::test_overrides::parallel_dispatch_result;
    use crate::daemon::run_once::{OfferDispatchTestOverrides, ParallelDispatchTestMode};

    let overrides =
        OfferDispatchTestOverrides::default().parallel_dispatch(ParallelDispatchTestMode::Success);
    let output = parallel_dispatch_result(&overrides)
        .expect("override configured")
        .expect("success output");
    assert_eq!(output.executed_count, 1);
}

#[test]
fn managed_post_override_success_returns_true() {
    use super::test_overrides::managed_post_result;
    use crate::daemon::run_once::{ManagedPostTestMode, OfferDispatchTestOverrides};

    let overrides =
        OfferDispatchTestOverrides::default().managed_post(ManagedPostTestMode::Success);
    let posted = managed_post_result(&overrides)
        .expect("override configured")
        .expect("success post");
    assert!(posted);
}
