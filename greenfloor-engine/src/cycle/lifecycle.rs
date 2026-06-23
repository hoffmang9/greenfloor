use serde::{Deserialize, Serialize};

use crate::cycle::reconcile::{REASON_POTENTIAL_TAKE_SEEN, REASON_TAKE_CONFIRMED_ON_TX_BLOCK};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OfferLifecycleState {
    Open,
    MempoolObserved,
    TxBlockConfirmed,
    RefreshDue,
    Expired,
}

impl OfferLifecycleState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::MempoolObserved => "mempool_observed",
            Self::TxBlockConfirmed => "tx_block_confirmed",
            Self::RefreshDue => "refresh_due",
            Self::Expired => "expired",
        }
    }

    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "open" => Some(Self::Open),
            "mempool_observed" => Some(Self::MempoolObserved),
            "tx_block_confirmed" => Some(Self::TxBlockConfirmed),
            "refresh_due" => Some(Self::RefreshDue),
            "expired" => Some(Self::Expired),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OfferSignal {
    MempoolSeen,
    TxConfirmed,
    ExpiryNear,
    Expired,
    RefreshPosted,
}

impl OfferSignal {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MempoolSeen => "mempool_seen",
            Self::TxConfirmed => "tx_confirmed",
            Self::ExpiryNear => "expiry_near",
            Self::Expired => "expired",
            Self::RefreshPosted => "refresh_posted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfferTransition {
    pub old_state: OfferLifecycleState,
    pub new_state: OfferLifecycleState,
    pub signal: OfferSignal,
    pub action: String,
    pub reason: String,
}

#[must_use]
pub fn apply_open_signal(signal: OfferSignal) -> OfferTransition {
    apply_offer_signal(OfferLifecycleState::Open, signal)
}

#[must_use]
pub fn apply_offer_signal(state: OfferLifecycleState, signal: OfferSignal) -> OfferTransition {
    match (state, signal) {
        (OfferLifecycleState::Open, OfferSignal::MempoolSeen) => OfferTransition {
            old_state: state,
            new_state: OfferLifecycleState::MempoolObserved,
            signal,
            action: "mark_mempool_observed".to_string(),
            reason: REASON_POTENTIAL_TAKE_SEEN.to_string(),
        },
        (
            OfferLifecycleState::Open | OfferLifecycleState::MempoolObserved,
            OfferSignal::TxConfirmed,
        ) => OfferTransition {
            old_state: state,
            new_state: OfferLifecycleState::TxBlockConfirmed,
            signal,
            action: "reconcile_coins_and_offers".to_string(),
            reason: REASON_TAKE_CONFIRMED_ON_TX_BLOCK.to_string(),
        },
        (OfferLifecycleState::Open, OfferSignal::ExpiryNear) => OfferTransition {
            old_state: state,
            new_state: OfferLifecycleState::RefreshDue,
            signal,
            action: "refresh_offer".to_string(),
            reason: "refresh_window_entered".to_string(),
        },
        (OfferLifecycleState::RefreshDue, OfferSignal::RefreshPosted) => OfferTransition {
            old_state: state,
            new_state: OfferLifecycleState::Open,
            signal,
            action: "track_new_offer_open".to_string(),
            reason: "offer_refreshed".to_string(),
        },
        (OfferLifecycleState::Open | OfferLifecycleState::RefreshDue, OfferSignal::Expired) => {
            OfferTransition {
                old_state: state,
                new_state: OfferLifecycleState::Expired,
                signal,
                action: "cleanup_offer_state".to_string(),
                reason: "offer_expired".to_string(),
            }
        }
        _ => OfferTransition {
            old_state: state,
            new_state: state,
            signal,
            action: "noop".to_string(),
            reason: "signal_ignored_for_state".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lifecycle_state_from_storage_string() {
        assert_eq!(
            OfferLifecycleState::parse("mempool_observed"),
            Some(OfferLifecycleState::MempoolObserved)
        );
        assert_eq!(OfferLifecycleState::parse("unknown"), None);
    }

    #[test]
    fn open_to_mempool_observed() {
        let transition = apply_offer_signal(OfferLifecycleState::Open, OfferSignal::MempoolSeen);
        assert_eq!(transition.new_state, OfferLifecycleState::MempoolObserved);
        assert_eq!(transition.action, "mark_mempool_observed");
    }

    #[test]
    fn refresh_posted_returns_to_open() {
        let transition =
            apply_offer_signal(OfferLifecycleState::RefreshDue, OfferSignal::RefreshPosted);
        assert_eq!(transition.new_state, OfferLifecycleState::Open);
        assert_eq!(transition.action, "track_new_offer_open");
    }

    #[test]
    fn noop_for_irrelevant_signal() {
        let transition = apply_offer_signal(
            OfferLifecycleState::TxBlockConfirmed,
            OfferSignal::MempoolSeen,
        );
        assert_eq!(transition.new_state, OfferLifecycleState::TxBlockConfirmed);
        assert_eq!(transition.action, "noop");
    }
}
