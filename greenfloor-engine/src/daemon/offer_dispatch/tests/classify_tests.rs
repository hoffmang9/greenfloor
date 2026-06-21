use std::collections::BTreeMap;

use tempfile::tempdir;

use super::super::{
    classify_parallel_dispatch, is_parallel_dispatch_transient_signer_error,
    record_parallel_fallback_audit, OfferDispatchOutput, ParallelDispatchDecision,
};
use crate::config::ManagerProgramConfig;
use crate::cycle::parallel_managed_dispatch_enabled;
use crate::error::SignerError;
use crate::storage::SqliteStore;

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
