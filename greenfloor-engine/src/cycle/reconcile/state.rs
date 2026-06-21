use std::borrow::Cow;

use crate::cycle::lifecycle::OfferLifecycleState;

pub(crate) const TAKER_NONE: &str = "none";
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReconcileState {
    Lifecycle(OfferLifecycleState),
    Cancelled,
    UnsupportedVenue,
}

impl ReconcileState {
    pub(crate) fn parse(raw: &str) -> Result<Self, ReconcileStateError> {
        match raw.trim() {
            "open" => Ok(Self::Lifecycle(OfferLifecycleState::Open)),
            "mempool_observed" => Ok(Self::Lifecycle(OfferLifecycleState::MempoolObserved)),
            "tx_block_confirmed" => Ok(Self::Lifecycle(OfferLifecycleState::TxBlockConfirmed)),
            "refresh_due" => Ok(Self::Lifecycle(OfferLifecycleState::RefreshDue)),
            "expired" => Ok(Self::Lifecycle(OfferLifecycleState::Expired)),
            "cancelled" => Ok(Self::Cancelled),
            STATE_UNSUPPORTED_VENUE => Ok(Self::UnsupportedVenue),
            other => Err(ReconcileStateError {
                state: other.to_string(),
            }),
        }
    }

    pub(crate) fn as_str(&self) -> Cow<'_, str> {
        match self {
            Self::Lifecycle(state) => Cow::Borrowed(state.as_str()),
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
}
