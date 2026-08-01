//! Pure ensure-candidate hash decision (no Coinset / post I/O).

use crate::hex::normalize_hex_id;

/// Local reuse choice before Coinset unspent checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EnsureReuseKind {
    PreferExisting,
    ReclaimThenNew,
}

#[must_use]
pub(super) fn decide_ensure_reuse(
    has_offer_nonce: bool,
    planned_hash: &str,
    stored_hash: &str,
) -> EnsureReuseKind {
    if has_offer_nonce && normalize_hex_id(planned_hash) == normalize_hex_id(stored_hash) {
        EnsureReuseKind::PreferExisting
    } else {
        EnsureReuseKind::ReclaimThenNew
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_reuse_prefers_existing_on_hash_match() {
        assert_eq!(
            decide_ensure_reuse(true, "aa".repeat(32).as_str(), &"aa".repeat(32)),
            EnsureReuseKind::PreferExisting
        );
    }

    #[test]
    fn ensure_reuse_reclaims_on_hash_mismatch_or_missing_nonce() {
        assert_eq!(
            decide_ensure_reuse(true, "aa".repeat(32).as_str(), &"bb".repeat(32)),
            EnsureReuseKind::ReclaimThenNew
        );
        assert_eq!(
            decide_ensure_reuse(false, "", &"aa".repeat(32)),
            EnsureReuseKind::ReclaimThenNew
        );
    }
}
