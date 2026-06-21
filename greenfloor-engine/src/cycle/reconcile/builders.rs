use std::borrow::Cow;

use crate::cycle::lifecycle::OfferSignal;

use super::metadata::{
    REASON_COINSET_MEMPOOL, REASON_DEXIE_OFFER_NOT_FOUND,
    REASON_DEXIE_OFFER_NOT_FOUND_PRESERVED_TERMINAL, REASON_OK, SIGNAL_SOURCE_COINSET_MEMPOOL,
    SIGNAL_SOURCE_DEXIE_GET_OFFER_404, SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK, SIGNAL_SOURCE_NONE,
    TAKER_DIAGNOSTIC_COINSET_MEMPOOL, TAKER_NONE,
};
use super::state::ReconcileState;
use super::transition::ReconcileTransition;

pub(crate) fn preserve_state(
    current_state: &ReconcileState,
    reason: &'static str,
) -> ReconcileTransition {
    ReconcileTransition::new(
        current_state.clone(),
        reason,
        SIGNAL_SOURCE_NONE,
        None,
        TAKER_NONE,
        TAKER_NONE,
    )
}

pub(crate) fn open_signal_transition(
    signal: OfferSignal,
    reason: &'static str,
    signal_source: &'static str,
    taker_signal: &'static str,
    taker_diagnostic: &'static str,
) -> ReconcileTransition {
    ReconcileTransition::new(
        ReconcileState::from_open_signal(signal),
        reason,
        signal_source,
        Some(signal),
        taker_signal,
        taker_diagnostic,
    )
}

pub(crate) fn dexie_fallback_transition(
    new_state: ReconcileState,
    signal: Option<OfferSignal>,
    taker_diagnostic: &'static str,
) -> ReconcileTransition {
    ReconcileTransition::new(
        new_state,
        REASON_OK,
        SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK,
        signal,
        TAKER_NONE,
        taker_diagnostic,
    )
}

pub(crate) fn preserve_mempool_observation(current_state: &ReconcileState) -> ReconcileTransition {
    ReconcileTransition::new(
        current_state.clone(),
        REASON_COINSET_MEMPOOL,
        SIGNAL_SOURCE_COINSET_MEMPOOL,
        None,
        TAKER_NONE,
        TAKER_DIAGNOSTIC_COINSET_MEMPOOL,
    )
}

pub(crate) fn unchanged(
    current_state: ReconcileState,
    reason: impl Into<Cow<'static, str>>,
) -> ReconcileTransition {
    ReconcileTransition::new(
        current_state,
        reason,
        SIGNAL_SOURCE_NONE,
        None,
        TAKER_NONE,
        TAKER_NONE,
    )
}

pub(crate) fn unsupported_venue(venue: &str) -> ReconcileTransition {
    ReconcileTransition::new(
        ReconcileState::UnsupportedVenue,
        Cow::Owned(format!("unsupported_venue:{venue}")),
        SIGNAL_SOURCE_NONE,
        None,
        TAKER_NONE,
        TAKER_NONE,
    )
}

pub(crate) fn missing_watched_offer_preserved(
    current_state: ReconcileState,
) -> ReconcileTransition {
    ReconcileTransition::new(
        current_state,
        REASON_DEXIE_OFFER_NOT_FOUND_PRESERVED_TERMINAL,
        SIGNAL_SOURCE_DEXIE_GET_OFFER_404,
        None,
        TAKER_NONE,
        TAKER_NONE,
    )
}

pub(crate) fn missing_watched_offer_expired() -> ReconcileTransition {
    ReconcileTransition::new(
        ReconcileState::from_open_signal(OfferSignal::Expired),
        REASON_DEXIE_OFFER_NOT_FOUND,
        SIGNAL_SOURCE_DEXIE_GET_OFFER_404,
        Some(OfferSignal::Expired),
        TAKER_NONE,
        TAKER_NONE,
    )
}
