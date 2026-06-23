//! Financial `offer_lifecycle_transition` rows preserved by audit retention.

use crate::cycle::lifecycle::OfferLifecycleState;

use super::metadata::{
    REASON_CANCEL_TX_CHAIN_CONFIRMED, REASON_COINSET_CONFIRMED, REASON_COINSET_MEMPOOL, REASON_OK,
    REASON_POTENTIAL_TAKE_SEEN, REASON_TAKE_CONFIRMED_ON_TX_BLOCK,
};
use super::state::STATE_CANCELLED;

pub(crate) const PRESERVED_LIFECYCLE_TRANSITIONS: &[(&str, &[&str])] = &[
    (
        OfferLifecycleState::MempoolObserved.as_str(),
        &[REASON_POTENTIAL_TAKE_SEEN, REASON_COINSET_MEMPOOL],
    ),
    (
        OfferLifecycleState::TxBlockConfirmed.as_str(),
        &[
            REASON_TAKE_CONFIRMED_ON_TX_BLOCK,
            REASON_COINSET_CONFIRMED,
            REASON_OK,
        ],
    ),
    (
        STATE_CANCELLED,
        &[REASON_CANCEL_TX_CHAIN_CONFIRMED, REASON_OK],
    ),
];

/// Canonical preserved `(new_state, reason)` pairs for financial lifecycle audit rows.
#[must_use]
pub fn preserved_lifecycle_transitions() -> &'static [(&'static str, &'static [&'static str])] {
    PRESERVED_LIFECYCLE_TRANSITIONS
}
