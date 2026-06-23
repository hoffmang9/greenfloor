//! Integration tests for offer dispatch wiring through `execute_strategy_actions`.
//! Injection branch tables are covered in `test_overrides::tests`.

use crate::daemon::dispatch_test_controls::{
    DaemonDispatchTestInjections, ManagedPostTestMode, ParallelDispatchTestMode,
};

use super::harness::{
    generous_spendable_profiles, sample_action, sample_market, sample_market_with_pricing,
    ParallelDispatchHarness,
};

#[tokio::test]
async fn execute_strategy_actions_parallel_disabled_uses_sequential_skip_path() {
    let harness = ParallelDispatchHarness::new(false, false, false);
    let output = harness
        .execute(&sample_market(), &[sample_action()])
        .await
        .expect("dispatch");

    assert_eq!(output.executed_count, 0);
    let events = harness
        .store
        .lock()
        .expect("lock")
        .list_recent_audit_events(Some(&["strategy_exec_skipped_no_signer"]), Some("m1"), 1)
        .expect("events");
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn execute_strategy_actions_parallel_transient_falls_back_to_sequential() {
    let mut harness = ParallelDispatchHarness::new(true, false, true);
    harness.set_offer_dispatch(
        DaemonDispatchTestInjections::default()
            .parallel(ParallelDispatchTestMode::Transient)
            .managed_post(ManagedPostTestMode::Success),
    );

    let output = harness
        .execute(&sample_market(), &[sample_action()])
        .await
        .expect("dispatch");
    assert_eq!(output.executed_count, 1);
}

#[tokio::test]
async fn execute_strategy_actions_parallel_fatal_propagates() {
    let mut harness = ParallelDispatchHarness::new(true, false, true);
    harness.set_offer_dispatch(
        DaemonDispatchTestInjections::default().parallel(ParallelDispatchTestMode::Fatal),
    );

    let err = harness
        .execute(&sample_market(), &[sample_action()])
        .await
        .expect_err("fatal parallel error");
    assert!(err.to_string().contains("permanent_offer_build_failure"));
}

#[tokio::test]
async fn execute_strategy_actions_parallel_success_short_circuits_prepare_path() {
    let mut harness = ParallelDispatchHarness::new(true, false, true);
    harness.set_offer_dispatch(
        DaemonDispatchTestInjections::default().parallel(ParallelDispatchTestMode::Success),
    );

    let output = harness
        .execute(&sample_market(), &[sample_action()])
        .await
        .expect("parallel shortcut dispatch");
    assert_eq!(output.executed_count, 1);
}

#[tokio::test]
async fn execute_strategy_actions_managed_post_success_via_sequential_path() {
    let mut harness = ParallelDispatchHarness::new(false, false, true);
    harness.set_offer_dispatch(
        DaemonDispatchTestInjections::default().managed_post(ManagedPostTestMode::Success),
    );

    let output = harness
        .execute(&sample_market(), &[sample_action()])
        .await
        .expect("dispatch");
    assert_eq!(output.executed_count, 1);
}

#[tokio::test]
async fn execute_strategy_actions_managed_post_failure_does_not_count_as_executed() {
    let mut harness = ParallelDispatchHarness::new(false, false, true);
    harness.set_offer_dispatch(
        DaemonDispatchTestInjections::default().managed_post(ManagedPostTestMode::Failure),
    );

    let output = harness
        .execute(&sample_market(), &[sample_action()])
        .await
        .expect("dispatch");
    assert_eq!(output.executed_count, 0);
}

#[tokio::test]
async fn execute_strategy_actions_parallel_success_runs_prepare_path() {
    let market = sample_market_with_pricing();
    let mut harness = ParallelDispatchHarness::new(true, false, true);
    let spendable_profiles = generous_spendable_profiles(&harness.program_path, &market).await;
    harness.set_offer_dispatch(
        DaemonDispatchTestInjections::default()
            .spendable_profiles(spendable_profiles)
            .managed_post(ManagedPostTestMode::Success),
    );

    let output = harness
        .execute(&market, &[sample_action()])
        .await
        .expect("parallel dispatch");
    assert_eq!(output.executed_count, 1);
}
