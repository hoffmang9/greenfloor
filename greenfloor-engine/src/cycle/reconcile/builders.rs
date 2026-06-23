use std::borrow::Cow;

use crate::cycle::lifecycle::OfferSignal;
use crate::offer::dexie_payload::{
    is_dexie_pattern_fallback_status, reconcile_from_dexie_status, DexieStatusReconcile,
};

use super::metadata::{
    REASON_CANCEL_TX_CHAIN_CONFIRMED, REASON_COINSET_MEMPOOL, REASON_DEXIE_OFFER_NOT_FOUND,
    REASON_DEXIE_OFFER_NOT_FOUND_PRESERVED_TERMINAL, REASON_OK, SIGNAL_SOURCE_CANCEL_TX_CHAIN,
    SIGNAL_SOURCE_COINSET_MEMPOOL, SIGNAL_SOURCE_DEXIE_GET_OFFER_404,
    SIGNAL_SOURCE_DEXIE_STATUS_FALLBACK, SIGNAL_SOURCE_NONE,
    TAKER_DIAGNOSTIC_CANCEL_TX_CHAIN_CONFIRMED, TAKER_DIAGNOSTIC_COINSET_MEMPOOL,
    TAKER_DIAGNOSTIC_DEXIE_PATTERN_FALLBACK, TAKER_NONE,
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

pub(crate) fn transition_from_dexie_status(
    status: i64,
    unchanged_state: ReconcileState,
) -> ReconcileTransition {
    let taker_diagnostic = if is_dexie_pattern_fallback_status(status) {
        TAKER_DIAGNOSTIC_DEXIE_PATTERN_FALLBACK
    } else {
        TAKER_NONE
    };
    match reconcile_from_dexie_status(status) {
        DexieStatusReconcile::Cancelled => {
            dexie_fallback_transition(ReconcileState::Cancelled, None, taker_diagnostic)
        }
        DexieStatusReconcile::ApplySignal(signal) => dexie_fallback_transition(
            ReconcileState::from_open_signal(signal),
            Some(signal),
            taker_diagnostic,
        ),
        DexieStatusReconcile::Unchanged => {
            dexie_fallback_transition(unchanged_state, None, taker_diagnostic)
        }
    }
}

pub(crate) fn cancel_tx_chain_confirmed_transition() -> ReconcileTransition {
    ReconcileTransition::new(
        ReconcileState::Cancelled,
        REASON_CANCEL_TX_CHAIN_CONFIRMED,
        SIGNAL_SOURCE_CANCEL_TX_CHAIN,
        None,
        TAKER_NONE,
        TAKER_DIAGNOSTIC_CANCEL_TX_CHAIN_CONFIRMED,
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
