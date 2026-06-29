//! Offer-creation gating after denomination bootstrap preflight.

use super::phase::{BootstrapPhaseSnapshot, BootstrapPhaseStatus};

const SKIP_CONTINUE_REASONS: &[&str] = &["already_ready", "dry_run"];

fn normalized_reason(reason: &str) -> String {
    let trimmed = reason.trim();
    if trimmed.is_empty() {
        "bootstrap_precheck_failed".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Typed bootstrap outcome for offer creation gating.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BootstrapOfferGate {
    Continue,
    BlockFailed(String),
    BlockPending(String),
    BlockSkipped(String),
}

impl BootstrapOfferGate {
    #[must_use]
    pub(crate) fn block_error(self) -> Option<String> {
        match self {
            Self::Continue => None,
            Self::BlockFailed(reason) => Some(format!("bootstrap_failed:{reason}")),
            Self::BlockPending(reason) => Some(format!("bootstrap_pending:{reason}")),
            Self::BlockSkipped(reason) => Some(format!("bootstrap_precheck_skipped:{reason}")),
        }
    }
}

/// Resolve whether offer creation should continue after a typed phase snapshot.
#[must_use]
pub(crate) fn bootstrap_offer_gate_for_snapshot(
    snapshot: &BootstrapPhaseSnapshot,
) -> BootstrapOfferGate {
    bootstrap_offer_gate_for_status(snapshot.status, &snapshot.reason, snapshot.ready)
}

/// Return manager bootstrap block reason text, or ``None`` when offer creation should continue.
#[must_use]
pub fn bootstrap_phase_snapshot_block_error(snapshot: &BootstrapPhaseSnapshot) -> Option<String> {
    bootstrap_offer_gate_for_snapshot(snapshot).block_error()
}

/// Resolve whether offer creation should continue after bootstrap preflight fields.
#[must_use]
pub(crate) fn bootstrap_offer_gate_for_status(
    status: BootstrapPhaseStatus,
    reason: &str,
    ready: bool,
) -> BootstrapOfferGate {
    let reason = normalized_reason(reason);
    match status {
        BootstrapPhaseStatus::Failed => BootstrapOfferGate::BlockFailed(reason),
        BootstrapPhaseStatus::Executed if !ready => BootstrapOfferGate::BlockPending(reason),
        BootstrapPhaseStatus::Skipped if !SKIP_CONTINUE_REASONS.contains(&reason.as_str()) => {
            BootstrapOfferGate::BlockSkipped(reason)
        }
        _ => BootstrapOfferGate::Continue,
    }
}

#[cfg(test)]
mod tests;
