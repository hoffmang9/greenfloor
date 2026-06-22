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
    CancelSubmitted,
    Cancelled,
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
            Self::CancelSubmitted => Cow::Borrowed("cancel_submitted"),
            Self::Cancelled => Cow::Borrowed("cancelled"),
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

    pub(crate) fn is_cancel_submitted(&self) -> bool {
        matches!(self, Self::CancelSubmitted)
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
