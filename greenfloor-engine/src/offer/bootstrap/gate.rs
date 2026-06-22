//! Offer-creation gating after denomination bootstrap preflight.

const SKIP_CONTINUE_REASONS: &[&str] = &["already_ready", "dry_run"];

fn normalized_reason(reason: &str) -> String {
    let trimmed = reason.trim();
    if trimmed.is_empty() {
        "bootstrap_precheck_failed".to_string()
    } else {
        trimmed.to_string()
    }
}

fn skip_allows_offer_creation(reason: &str) -> bool {
    SKIP_CONTINUE_REASONS.contains(&normalized_reason(reason).as_str())
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

/// Resolve whether offer creation should continue after bootstrap preflight.
#[must_use]
pub(crate) fn bootstrap_offer_gate(status: &str, reason: &str, ready: bool) -> BootstrapOfferGate {
    let reason = normalized_reason(reason);
    match status.trim() {
        "failed" => BootstrapOfferGate::BlockFailed(reason),
        "executed" if !ready => BootstrapOfferGate::BlockPending(reason),
        "skipped" if !skip_allows_offer_creation(&reason) => {
            BootstrapOfferGate::BlockSkipped(reason)
        }
        _ => BootstrapOfferGate::Continue,
    }
}

#[cfg(test)]
mod tests;
