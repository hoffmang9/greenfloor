use super::{
    bootstrap_offer_gate_for_snapshot, bootstrap_offer_gate_for_status, BootstrapOfferGate,
    BootstrapPhaseStatus,
};
use crate::offer::bootstrap::BootstrapPhaseSnapshot;

#[test]
fn snapshot_gate_typed_outcomes() {
    assert_eq!(
        bootstrap_offer_gate_for_snapshot(&BootstrapPhaseSnapshot {
            status: "failed",
            reason: "split_error".to_string(),
            ready: false,
        }),
        BootstrapOfferGate::BlockFailed("split_error".to_string())
    );
    assert_eq!(
        bootstrap_offer_gate_for_snapshot(&BootstrapPhaseSnapshot {
            status: "executed",
            reason: "split_submitted".to_string(),
            ready: false,
        }),
        BootstrapOfferGate::BlockPending("split_submitted".to_string())
    );
    assert_eq!(
        bootstrap_offer_gate_for_snapshot(&BootstrapPhaseSnapshot {
            status: "skipped",
            reason: "already_ready".to_string(),
            ready: false,
        }),
        BootstrapOfferGate::Continue
    );
    assert_eq!(
        bootstrap_offer_gate_for_snapshot(&BootstrapPhaseSnapshot {
            status: "skipped",
            reason: "dry_run".to_string(),
            ready: false,
        }),
        BootstrapOfferGate::Continue
    );
    assert_eq!(
        bootstrap_offer_gate_for_snapshot(&BootstrapPhaseSnapshot {
            status: "skipped",
            reason: "seed_missing".to_string(),
            ready: false,
        }),
        BootstrapOfferGate::BlockSkipped("seed_missing".to_string())
    );
}

#[test]
fn snapshot_gate_matches_typed_status_gate() {
    let snapshot = BootstrapPhaseSnapshot {
        status: "executed",
        reason: "split_submitted".to_string(),
        ready: false,
    };
    assert_eq!(
        bootstrap_offer_gate_for_snapshot(&snapshot),
        bootstrap_offer_gate_for_status(
            BootstrapPhaseStatus::from_snapshot_status(snapshot.status),
            &snapshot.reason,
            snapshot.ready
        )
    );
}

#[test]
fn block_error_for_failed_pending_and_skipped() {
    assert_eq!(
        bootstrap_offer_gate_for_status(BootstrapPhaseStatus::Failed, "split_error", false)
            .block_error(),
        Some("bootstrap_failed:split_error".to_string())
    );
    assert_eq!(
        bootstrap_offer_gate_for_status(BootstrapPhaseStatus::Executed, "split_submitted", false)
            .block_error(),
        Some("bootstrap_pending:split_submitted".to_string())
    );
    assert_eq!(
        bootstrap_offer_gate_for_status(BootstrapPhaseStatus::Skipped, "seed_missing", false)
            .block_error(),
        Some("bootstrap_precheck_skipped:seed_missing".to_string())
    );
}

#[test]
fn block_error_allows_ready_skip_reasons() {
    assert_eq!(
        bootstrap_offer_gate_for_status(BootstrapPhaseStatus::Skipped, "already_ready", false)
            .block_error(),
        None
    );
    assert_eq!(
        bootstrap_offer_gate_for_status(BootstrapPhaseStatus::Skipped, "dry_run", false)
            .block_error(),
        None
    );
}

#[test]
fn block_error_uses_default_reason_when_missing() {
    assert_eq!(
        bootstrap_offer_gate_for_status(BootstrapPhaseStatus::Failed, "", false).block_error(),
        Some("bootstrap_failed:bootstrap_precheck_failed".to_string())
    );
}
