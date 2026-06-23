//! Integration tests for offer dispatch wiring through `execute_strategy_actions`.
//! Injection branch tables are covered in `test_overrides::tests`.

use crate::daemon::dispatch_test_controls::{
    DaemonDispatchTestInjections, ManagedPostTestMode, ParallelDispatchTestMode,
};

use crate::storage::lock_shared_store_for_test;

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
    let events = lock_shared_store_for_test(&harness.store)
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

#[tokio::test]
async fn parallel_dispatch_persist_flush_does_not_reopen_cycle_db() {
    use crate::storage::{reset_sqlite_open_calls_for_test, sqlite_open_calls_for_test};

    let market = sample_market_with_pricing();
    let mut harness = ParallelDispatchHarness::new(true, false, true);
    reset_sqlite_open_calls_for_test();
    let spendable_profiles = generous_spendable_profiles(&harness.program_path, &market).await;
    harness.set_offer_dispatch(
        DaemonDispatchTestInjections::default()
            .spendable_profiles(spendable_profiles)
            .managed_post(ManagedPostTestMode::ExerciseSharedPersistFlush),
    );

    let output = harness
        .execute(&market, &[sample_action()])
        .await
        .expect("parallel persist dispatch");
    assert_eq!(output.executed_count, 1);
    assert_eq!(sqlite_open_calls_for_test(), 0);
}
