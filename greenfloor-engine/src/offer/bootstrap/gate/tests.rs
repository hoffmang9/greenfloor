use super::{bootstrap_offer_gate, BootstrapOfferGate};
use crate::offer::bootstrap::BootstrapPhaseSnapshot;

#[test]
fn bootstrap_offer_gate_typed_outcomes() {
    assert_eq!(
        bootstrap_offer_gate("failed", "split_error", false),
        BootstrapOfferGate::BlockFailed("split_error".to_string())
    );
    assert_eq!(
        bootstrap_offer_gate("executed", "split_submitted", false),
        BootstrapOfferGate::BlockPending("split_submitted".to_string())
    );
    assert_eq!(
        bootstrap_offer_gate("skipped", "already_ready", false),
        BootstrapOfferGate::Continue
    );
    assert_eq!(
        bootstrap_offer_gate("skipped", "dry_run", false),
        BootstrapOfferGate::Continue
    );
    assert_eq!(
        bootstrap_offer_gate("skipped", "seed_missing", false),
        BootstrapOfferGate::BlockSkipped("seed_missing".to_string())
    );
}

#[test]
fn bootstrap_offer_gate_matches_typed_phase_snapshot() {
    let snapshot = BootstrapPhaseSnapshot {
        status: "executed",
        reason: "split_submitted".to_string(),
        ready: false,
    };
    assert_eq!(
        bootstrap_offer_gate(snapshot.status, &snapshot.reason, snapshot.ready),
        BootstrapOfferGate::BlockPending("split_submitted".to_string())
    );
}

#[test]
fn block_error_for_failed_pending_and_skipped() {
    assert_eq!(
        bootstrap_offer_gate("failed", "split_error", false).block_error(),
        Some("bootstrap_failed:split_error".to_string())
    );
    assert_eq!(
        bootstrap_offer_gate("executed", "split_submitted", false).block_error(),
        Some("bootstrap_pending:split_submitted".to_string())
    );
    assert_eq!(
        bootstrap_offer_gate("skipped", "seed_missing", false).block_error(),
        Some("bootstrap_precheck_skipped:seed_missing".to_string())
    );
}

#[test]
fn block_error_allows_ready_skip_reasons() {
    assert_eq!(
        bootstrap_offer_gate("skipped", "already_ready", false).block_error(),
        None
    );
    assert_eq!(
        bootstrap_offer_gate("skipped", "dry_run", false).block_error(),
        None
    );
}

#[test]
fn block_error_uses_default_reason_when_missing() {
    assert_eq!(
        bootstrap_offer_gate("failed", "", false).block_error(),
        Some("bootstrap_failed:bootstrap_precheck_failed".to_string())
    );
}
