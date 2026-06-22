use std::path::PathBuf;

use crate::cycle::StaleSweepProgress;

use super::*;

#[test]
fn compute_cycle_exit_code_non_zero_when_all_markets_fail() {
    let plan = CyclePlan {
        enabled_market_ids: vec!["m1".to_string()],
        selected_market_ids: vec!["m1".to_string()],
        consumed_immediate_requeues: Vec::new(),
        dispatch_state: DaemonDispatchState::default(),
        stale_open_sweep: StaleSweepProgress::default(),
        configured_market_slot_count: 1,
        runtime_dry_run: false,
        db_path: PathBuf::from("/tmp/db.sqlite"),
        previous_xch_price_usd: None,
        dexie_base_url: String::new(),
        splash_base_url: String::new(),
        test_controls: DaemonCycleTestControls::default(),
    };
    let metrics = MarketDispatchMetrics {
        markets_processed: 0,
        cycle_error_count: 1,
        ..MarketDispatchMetrics::default()
    };
    assert_eq!(compute_cycle_exit_code(&plan, &metrics), 1);
}

#[test]
fn resolve_state_db_path_prefers_explicit_override() {
    use crate::storage::state_db_path_for_home;

    let home = PathBuf::from("/tmp/gf");
    assert_eq!(
        crate::storage::resolve_state_db_path(&home, Some("/tmp/custom.sqlite")),
        PathBuf::from("/tmp/custom.sqlite")
    );
    assert_eq!(
        crate::storage::resolve_state_db_path(&home, None),
        state_db_path_for_home(&home)
    );
}

#[test]
fn test_controls_default_allowed_without_env_gate() {
    std::env::set_var("GREENFLOOR_DAEMON_TEST_CONTROLS", "0");
    let controls = DaemonCycleTestControls::default();
    assert!(controls.ensure_allowed().is_ok());
}

#[test]
fn test_controls_non_default_rejected_without_env_gate() {
    std::env::set_var("GREENFLOOR_DAEMON_TEST_CONTROLS", "0");
    let controls = DaemonCycleTestControls {
        skip_strategy_execution: true,
        force_market_error_for: None,
        ..Default::default()
    };
    let err = controls.ensure_allowed().expect_err("gate");
    assert!(err
        .to_string()
        .contains("GREENFLOOR_DAEMON_TEST_CONTROLS=1"));
}

#[test]
fn test_controls_non_default_allowed_when_env_gate_set() {
    std::env::set_var("GREENFLOOR_DAEMON_TEST_CONTROLS", "1");
    let controls = DaemonCycleTestControls {
        skip_strategy_execution: true,
        force_market_error_for: Some("m1".to_string()),
        ..Default::default()
    };
    assert!(controls.ensure_allowed().is_ok());
    std::env::remove_var("GREENFLOOR_DAEMON_TEST_CONTROLS");
}
