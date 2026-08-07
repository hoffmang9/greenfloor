use std::borrow::Cow;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::cycle::lifecycle::{apply_open_signal, OfferLifecycleState, OfferSignal};

pub(crate) const STATE_UNSUPPORTED_VENUE: &str = "reconcile_unsupported_venue";
pub(crate) const STATE_CANCELLED: &str = "cancelled";
/// In-flight CAS lock for expired-maker `PreferExisting` / reclaim (not a Dexie cancel).
pub(crate) const STATE_MAKER_CLAIMED: &str = "maker_claimed";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileStateError {
    state: String,
}

impl std::fmt::Display for ReconcileStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown offer reconcile state: {}", self.state)
    }
}

impl std::error::Error for ReconcileStateError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileState {
    Lifecycle(OfferLifecycleState),
    PendingVisibility,
    CancelSubmitted,
    Cancelled,
    /// Exclusive claim on an expired maker during ensure/soft-expire I/O.
    MakerClaimed,
    UnknownOrphaned,
    UnsupportedVenue,
}

impl ReconcileState {
    /// Parse a persisted offer state string into a typed reconcile state.
    ///
    /// # Errors
    ///
    /// Returns an error when `raw` is not a known lifecycle or reconcile-only state.
    pub fn parse(raw: &str) -> Result<Self, ReconcileStateError> {
        let trimmed = raw.trim();
        if trimmed == STATE_CANCELLED {
            return Ok(Self::Cancelled);
        }
        if trimmed == STATE_MAKER_CLAIMED {
            return Ok(Self::MakerClaimed);
        }
        if trimmed == "cancel_submitted" {
            return Ok(Self::CancelSubmitted);
        }
        if trimmed == "pending_visibility" {
            return Ok(Self::PendingVisibility);
        }
        if trimmed == "unknown_orphaned" {
            return Ok(Self::UnknownOrphaned);
        }
        if trimmed == STATE_UNSUPPORTED_VENUE {
            return Ok(Self::UnsupportedVenue);
        }
        OfferLifecycleState::parse(trimmed)
            .map(Self::Lifecycle)
            .ok_or_else(|| ReconcileStateError {
                state: trimmed.to_string(),
            })
    }

    #[must_use]
    pub fn from_open_signal(signal: OfferSignal) -> Self {
        Self::Lifecycle(apply_open_signal(signal).new_state)
    }

    #[must_use]
    pub fn as_str(&self) -> Cow<'_, str> {
        match self {
            Self::Lifecycle(state) => Cow::Borrowed(state.as_str()),
            Self::PendingVisibility => Cow::Borrowed("pending_visibility"),
            Self::CancelSubmitted => Cow::Borrowed("cancel_submitted"),
            Self::Cancelled => Cow::Borrowed(STATE_CANCELLED),
            Self::MakerClaimed => Cow::Borrowed(STATE_MAKER_CLAIMED),
            Self::UnknownOrphaned => Cow::Borrowed("unknown_orphaned"),
            Self::UnsupportedVenue => Cow::Borrowed(STATE_UNSUPPORTED_VENUE),
        }
    }

    pub(crate) fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Lifecycle(OfferLifecycleState::TxBlockConfirmed | OfferLifecycleState::Expired)
                | Self::Cancelled
        )
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }

    #[must_use]
    pub fn is_cancel_submitted(&self) -> bool {
        matches!(self, Self::CancelSubmitted)
    }

    /// Whether a tracked offer in this state is eligible for operator-initiated cancel.
    #[must_use]
    pub fn is_cancel_eligible(&self) -> bool {
        matches!(
            self,
            Self::Lifecycle(OfferLifecycleState::Open) | Self::PendingVisibility
        )
    }

    /// Whether offers in this state stay on the daemon reconcile watchlist.
    #[must_use]
    pub fn is_watched_for_reconcile(&self) -> bool {
        match self {
            Self::Lifecycle(
                OfferLifecycleState::Open
                | OfferLifecycleState::RefreshDue
                | OfferLifecycleState::MempoolObserved,
            )
            | Self::PendingVisibility
            | Self::CancelSubmitted
            | Self::UnknownOrphaned => true,
            Self::Lifecycle(_) | Self::Cancelled | Self::MakerClaimed | Self::UnsupportedVenue => {
                false
            }
        }
    }

    /// Whether this state always occupies a ladder capacity slot (no age gate).
    ///
    /// Distinct from [`Self::is_watched_for_reconcile`]: `maker_claimed` holds capacity
    /// during ensure/soft-expire I/O but is not Dexie-reconciled. `mempool_observed` is
    /// capacity-eligible only when recent (caller applies the age predicate).
    #[must_use]
    pub fn counts_toward_ladder_capacity(&self) -> bool {
        matches!(
            self,
            Self::Lifecycle(OfferLifecycleState::Open | OfferLifecycleState::RefreshDue)
                | Self::MakerClaimed
        )
    }

    /// Whether capacity counting may include this state when `updated_at` is recent.
    #[must_use]
    pub fn is_timed_ladder_capacity_candidate(&self) -> bool {
        matches!(self, Self::Lifecycle(OfferLifecycleState::MempoolObserved))
    }

    /// Whether this state's maker coin must stay excluded from a new Direct unique pin.
    ///
    /// Watched reconcile rows plus `maker_claimed` (capacity lock, not Dexie-watched).
    #[must_use]
    pub fn binds_unique_maker_coin(&self) -> bool {
        self.is_watched_for_reconcile() || matches!(self, Self::MakerClaimed)
    }
}

/// Persistable states loaded for ladder capacity (includes mempool for timed filter).
pub(crate) const LADDER_CAPACITY_QUERY_STATES: &[&str] = &[
    "open",
    "refresh_due",
    STATE_MAKER_CLAIMED,
    "mempool_observed",
];

/// Persistable states whose `cancel_input_coin_id` binds a Direct maker (ADR 0022).
///
/// Must stay aligned with [`ReconcileState::binds_unique_maker_coin`].
pub(crate) const BINDING_MAKER_QUERY_STATES: &[&str] = &[
    "open",
    "refresh_due",
    "mempool_observed",
    "pending_visibility",
    "cancel_submitted",
    "unknown_orphaned",
    STATE_MAKER_CLAIMED,
];

impl Serialize for ReconcileState {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_str())
    }
}

impl<'de> Deserialize<'de> for ReconcileState {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pending_visibility_and_unknown_orphaned() {
        assert_eq!(
            ReconcileState::parse("pending_visibility"),
            Ok(ReconcileState::PendingVisibility)
        );
        assert_eq!(
            ReconcileState::parse("unknown_orphaned"),
            Ok(ReconcileState::UnknownOrphaned)
        );
    }

    #[test]
    fn cancel_eligible_states() {
        assert!(ReconcileState::Lifecycle(OfferLifecycleState::Open).is_cancel_eligible());
        assert!(ReconcileState::PendingVisibility.is_cancel_eligible());
        assert!(!ReconcileState::CancelSubmitted.is_cancel_eligible());
    }

    #[test]
    fn ladder_capacity_includes_maker_claimed_not_reconcile_watch() {
        assert!(ReconcileState::MakerClaimed.counts_toward_ladder_capacity());
        assert!(!ReconcileState::MakerClaimed.is_watched_for_reconcile());
        assert!(
            ReconcileState::Lifecycle(OfferLifecycleState::MempoolObserved)
                .is_timed_ladder_capacity_candidate()
        );
        assert!(
            !ReconcileState::Lifecycle(OfferLifecycleState::MempoolObserved)
                .counts_toward_ladder_capacity()
        );
    }

    #[test]
    fn binds_unique_maker_includes_watched_and_maker_claimed() {
        assert!(ReconcileState::MakerClaimed.binds_unique_maker_coin());
        assert!(ReconcileState::PendingVisibility.binds_unique_maker_coin());
        assert!(ReconcileState::CancelSubmitted.binds_unique_maker_coin());
        assert!(ReconcileState::UnknownOrphaned.binds_unique_maker_coin());
        assert!(ReconcileState::Lifecycle(OfferLifecycleState::Open).binds_unique_maker_coin());
        assert!(!ReconcileState::Cancelled.binds_unique_maker_coin());
        assert!(!ReconcileState::Lifecycle(OfferLifecycleState::Expired).binds_unique_maker_coin());
    }

    #[test]
    fn binding_maker_query_states_match_predicate() {
        for raw in BINDING_MAKER_QUERY_STATES {
            let parsed = ReconcileState::parse(raw).expect("query state parses");
            assert!(
                parsed.binds_unique_maker_coin(),
                "{raw} must bind unique makers"
            );
        }
    }
}
