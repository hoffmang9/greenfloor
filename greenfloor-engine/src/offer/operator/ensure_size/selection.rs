//! Pure ensure-candidate hash decision (no Coinset / post I/O).

use crate::hex::normalize_hex_id;

/// Pure selection among reusable makers before Coinset/post I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EnsureCandidateDecision {
    PreferExisting,
    ReclaimThenNew,
    /// Maker coin is spent; retire the expired listing without reclaim spend.
    RetireSpent,
}

#[must_use]
pub(super) fn decide_ensure_candidate(
    unspent: bool,
    has_offer_nonce: bool,
    planned_hash: &str,
    stored_hash: &str,
) -> EnsureCandidateDecision {
    if !unspent {
        return EnsureCandidateDecision::RetireSpent;
    }
    if !has_offer_nonce {
        return EnsureCandidateDecision::ReclaimThenNew;
    }
    if normalize_hex_id(planned_hash) == normalize_hex_id(stored_hash) {
        EnsureCandidateDecision::PreferExisting
    } else {
        EnsureCandidateDecision::ReclaimThenNew
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_candidate_prefers_existing_on_hash_match() {
        assert_eq!(
            decide_ensure_candidate(true, true, "aa".repeat(32).as_str(), &"aa".repeat(32)),
            EnsureCandidateDecision::PreferExisting
        );
    }

    #[test]
    fn ensure_candidate_reclaims_on_hash_mismatch_or_missing_nonce() {
        assert_eq!(
            decide_ensure_candidate(true, true, "aa".repeat(32).as_str(), &"bb".repeat(32)),
            EnsureCandidateDecision::ReclaimThenNew
        );
        assert_eq!(
            decide_ensure_candidate(true, false, "", &"aa".repeat(32)),
            EnsureCandidateDecision::ReclaimThenNew
        );
    }

    #[test]
    fn ensure_candidate_retires_spent_makers() {
        assert_eq!(
            decide_ensure_candidate(false, true, "aa".repeat(32).as_str(), &"aa".repeat(32)),
            EnsureCandidateDecision::RetireSpent
        );
    }
}
