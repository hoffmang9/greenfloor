//! Pure ensure-candidate selection (no Coinset / post I/O).

use crate::hex::normalize_hex_id;

/// Pure selection among reusable makers before Coinset/post I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EnsureCandidateDecision {
    PreferExisting,
    ReclaimThenNew,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EnsureSelection {
    PreferExisting(usize),
    ReclaimThenNew(usize),
    New,
}

#[must_use]
pub(super) fn decide_ensure_candidate(
    unspent: bool,
    has_offer_nonce: bool,
    planned_hash: &str,
    stored_hash: &str,
) -> Option<EnsureCandidateDecision> {
    if !unspent {
        return None;
    }
    if !has_offer_nonce {
        return Some(EnsureCandidateDecision::ReclaimThenNew);
    }
    Some(
        if normalize_hex_id(planned_hash) == normalize_hex_id(stored_hash) {
            EnsureCandidateDecision::PreferExisting
        } else {
            EnsureCandidateDecision::ReclaimThenNew
        },
    )
}

/// Short-circuit on `PreferExisting`; otherwise first reclaim index or New.
#[must_use]
pub(super) fn select_from_decisions(
    decisions: &[Option<EnsureCandidateDecision>],
) -> EnsureSelection {
    let mut reclaim_idx = None;
    for (idx, decision) in decisions.iter().enumerate() {
        match decision {
            Some(EnsureCandidateDecision::PreferExisting) => {
                return EnsureSelection::PreferExisting(idx);
            }
            Some(EnsureCandidateDecision::ReclaimThenNew) if reclaim_idx.is_none() => {
                reclaim_idx = Some(idx);
            }
            _ => {}
        }
    }
    reclaim_idx.map_or(EnsureSelection::New, EnsureSelection::ReclaimThenNew)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_candidate_prefers_existing_on_hash_match() {
        assert_eq!(
            decide_ensure_candidate(true, true, "aa".repeat(32).as_str(), &"aa".repeat(32)),
            Some(EnsureCandidateDecision::PreferExisting)
        );
    }

    #[test]
    fn ensure_candidate_reclaims_on_hash_mismatch_or_missing_nonce() {
        assert_eq!(
            decide_ensure_candidate(true, true, "aa".repeat(32).as_str(), &"bb".repeat(32)),
            Some(EnsureCandidateDecision::ReclaimThenNew)
        );
        assert_eq!(
            decide_ensure_candidate(true, false, "", &"aa".repeat(32)),
            Some(EnsureCandidateDecision::ReclaimThenNew)
        );
    }

    #[test]
    fn ensure_candidate_skips_spent_makers() {
        assert_eq!(
            decide_ensure_candidate(false, true, "aa".repeat(32).as_str(), &"aa".repeat(32)),
            None
        );
    }

    #[test]
    fn select_short_circuits_on_prefer_even_after_reclaim_candidate() {
        let decisions = [
            Some(EnsureCandidateDecision::ReclaimThenNew),
            Some(EnsureCandidateDecision::PreferExisting),
            Some(EnsureCandidateDecision::ReclaimThenNew),
        ];
        assert_eq!(
            select_from_decisions(&decisions),
            EnsureSelection::PreferExisting(1)
        );
    }

    #[test]
    fn select_uses_first_reclaim_when_no_prefer() {
        let decisions = [
            None,
            Some(EnsureCandidateDecision::ReclaimThenNew),
            Some(EnsureCandidateDecision::ReclaimThenNew),
        ];
        assert_eq!(
            select_from_decisions(&decisions),
            EnsureSelection::ReclaimThenNew(1)
        );
    }

    #[test]
    fn select_new_when_all_spent() {
        assert_eq!(select_from_decisions(&[None, None]), EnsureSelection::New);
    }
}
