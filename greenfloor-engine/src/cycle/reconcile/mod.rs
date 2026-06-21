//! Offer transition policy for daemon cycle signals (taker detection, venue rules).
//!
//! Batch DB reconcile lives in `offer::lifecycle::reconcile_watched_offers`; per-market
//! cycle reconcile lives in `daemon::reconcile_market_cycle`.

mod decision;
mod metadata;
mod state;
mod transition;

#[cfg(test)]
mod tests;

pub use state::{ReconcileState, ReconcileStateError};
pub use transition::CycleOfferTransition;

use std::borrow::Cow;

use crate::cycle::lifecycle::OfferSignal;

use decision::resolve_watched_offer_decision;
use metadata::{
    REASON_DEXIE_OFFER_NOT_FOUND, REASON_DEXIE_OFFER_NOT_FOUND_PRESERVED_TERMINAL,
    SIGNAL_SOURCE_DEXIE_GET_OFFER_404, SIGNAL_SOURCE_NONE, TAKER_NONE,
};
use transition::ReconcileTransition;

/// Unchanged offer transition.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn unchanged_offer_transition(
    current_state: &str,
    reason: impl Into<String>,
) -> Result<CycleOfferTransition, ReconcileStateError> {
    let old_state = ReconcileState::parse(current_state)?;
    Ok(ReconcileTransition::new(
        old_state.clone(),
        Cow::Owned(reason.into()),
        SIGNAL_SOURCE_NONE,
        None,
        TAKER_NONE,
        TAKER_NONE,
    )
    .into_cycle_transition_no_coinset(old_state))
}

/// Unsupported venue offer transition.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn unsupported_venue_offer_transition(
    current_state: &str,
    venue: &str,
) -> Result<CycleOfferTransition, ReconcileStateError> {
    let old_state = ReconcileState::parse(current_state)?;
    Ok(ReconcileTransition::new(
        ReconcileState::UnsupportedVenue,
        Cow::Owned(format!("unsupported_venue:{venue}")),
        SIGNAL_SOURCE_NONE,
        None,
        TAKER_NONE,
        TAKER_NONE,
    )
    .into_cycle_transition_no_coinset(old_state))
}

/// Resolve missing watched offer transition.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn resolve_missing_watched_offer_transition(
    current_state: &str,
) -> Result<CycleOfferTransition, ReconcileStateError> {
    let old_state = ReconcileState::parse(current_state)?;
    if old_state.is_terminal() {
        return Ok(ReconcileTransition::new(
            old_state.clone(),
            REASON_DEXIE_OFFER_NOT_FOUND_PRESERVED_TERMINAL,
            SIGNAL_SOURCE_DEXIE_GET_OFFER_404,
            None,
            TAKER_NONE,
            TAKER_NONE,
        )
        .into_cycle_transition_no_coinset(old_state));
    }
    Ok(ReconcileTransition::new(
        ReconcileState::from_open_signal(OfferSignal::Expired),
        REASON_DEXIE_OFFER_NOT_FOUND,
        SIGNAL_SOURCE_DEXIE_GET_OFFER_404,
        Some(OfferSignal::Expired),
        TAKER_NONE,
        TAKER_NONE,
    )
    .into_cycle_transition_no_coinset(old_state))
}

/// Resolve watched offer transition from signals.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn resolve_watched_offer_transition_from_signals(
    current_state: &str,
    status: Option<i64>,
    coinset_tx_ids: Vec<String>,
    coinset_confirmed_tx_ids: Vec<String>,
    coinset_mempool_tx_ids: Vec<String>,
) -> Result<CycleOfferTransition, ReconcileStateError> {
    let old_state = ReconcileState::parse(current_state)?;
    Ok(resolve_watched_offer_decision(
        &old_state,
        status,
        &coinset_tx_ids,
        &coinset_confirmed_tx_ids,
        &coinset_mempool_tx_ids,
    )
    .into_cycle_transition(
        old_state,
        coinset_tx_ids,
        coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids,
    ))
}
