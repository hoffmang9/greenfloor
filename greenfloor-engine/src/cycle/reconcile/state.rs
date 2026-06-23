use std::borrow::Cow;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::cycle::lifecycle::{apply_open_signal, OfferLifecycleState, OfferSignal};

pub(crate) const STATE_UNSUPPORTED_VENUE: &str = "reconcile_unsupported_venue";

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
        if trimmed == "cancelled" {
            return Ok(Self::Cancelled);
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
            Self::Cancelled => Cow::Borrowed("cancelled"),
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
            Self::Lifecycle(_) | Self::Cancelled | Self::UnsupportedVenue => false,
        }
    }
}

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
}
