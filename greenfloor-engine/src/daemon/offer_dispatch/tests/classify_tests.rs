use std::collections::BTreeMap;

use tempfile::tempdir;

use super::super::{
    classify_parallel_dispatch, parallel_managed_dispatch_enabled, parallel_max_workers,
    record_parallel_fallback_audit, reservation_release_status, OfferDispatchOutput,
    ParallelDispatchDecision,
};
use crate::config::ManagerProgramConfig;
use crate::error::SignerError;
use crate::storage::{lock_shared_store_for_test, CycleWriteStore};

#[test]
fn parallel_managed_dispatch_enabled_requires_parallelism_and_live_runtime() {
    let mut program = ManagerProgramConfig {
        runtime_market_slot_count: 1,
        runtime_offer_parallelism_enabled: true,
        runtime_offer_parallelism_max_workers: 2,
        tx_block_websocket_reconnect_interval_seconds: 1,
        tx_block_fallback_poll_interval_seconds: 1,
        ..Default::default()
    };
    assert!(parallel_managed_dispatch_enabled(&program));
    program.runtime_offer_parallelism_enabled = false;
    assert!(!parallel_managed_dispatch_enabled(&program));
    program.runtime_offer_parallelism_enabled = true;
    program.runtime_dry_run = true;
    assert!(!parallel_managed_dispatch_enabled(&program));
}

#[test]
fn parallel_max_workers_caps_at_submission_count() {
    assert_eq!(parallel_max_workers(3, 8), 3);
    assert_eq!(parallel_max_workers(0, 0), 0);
    assert_eq!(parallel_max_workers(5, 0), 1);
}

#[test]
fn reservation_release_status_reflects_execution_outcome() {
    assert_eq!(reservation_release_status(true), "released_success");
    assert_eq!(reservation_release_status(false), "released_failed");
}

#[test]
fn parallel_transient_signer_error_classifies_reservation_and_upstream() {
    let contention = SignerError::ReservationContention("busy".to_string());
    assert!(contention.is_parallel_dispatch_transient());
    let upstream = SignerError::ManagedUpstreamTransient("timeout".to_string());
    assert!(upstream.is_parallel_dispatch_transient());
    let locked = SignerError::DatabaseLocked;
    assert!(locked.is_parallel_dispatch_transient());
    let fatal = SignerError::Other("permanent_offer_build_failure: bad puzzle".to_string());
    assert!(!fatal.is_parallel_dispatch_transient());
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

#[tokio::test]
async fn record_parallel_fallback_audit_persists_event() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("greenfloor.sqlite");
    let store = CycleWriteStore::open(&db_path).expect("open");
    let err = SignerError::Other("ReservationContentionError: simulated".to_string());
    record_parallel_fallback_audit(&store, "m1", &err).expect("audit");
    let events = lock_shared_store_for_test(&store)
        .list_recent_audit_events(Some(&["offer_parallel_fallback"]), Some("m1"), 5)
        .expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "offer_parallel_fallback");
}
