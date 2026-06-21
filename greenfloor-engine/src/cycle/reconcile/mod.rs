//! Offer transition policy for daemon cycle signals (taker detection, venue rules).
//!
//! Batch DB reconcile lives in `offer::lifecycle::reconcile_watched_offers`; per-market
//! cycle reconcile lives in `daemon::reconcile_market_cycle`.

mod decision;
mod state;
mod transition;

#[cfg(test)]
mod tests;

pub use state::{ReconcileState, ReconcileStateError};
pub use transition::CycleOfferTransition;

use std::borrow::Cow;

use crate::cycle::lifecycle::OfferSignal;

use decision::resolve_watched_offer_decision;
use state::TAKER_NONE;
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
    Ok(ReconcileTransition::with_owned_reason(
        old_state.clone(),
        Cow::Owned(reason.into()),
        "none",
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
    Ok(ReconcileTransition::with_owned_reason(
        ReconcileState::UnsupportedVenue,
        Cow::Owned(format!("unsupported_venue:{venue}")),
        "none",
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
            "dexie_offer_not_found_preserved_terminal",
            "dexie_get_offer_404",
            None,
            TAKER_NONE,
            TAKER_NONE,
        )
        .into_cycle_transition_no_coinset(old_state));
    }
    let (new_state, signal) = ReconcileState::from_open_signal(OfferSignal::Expired);
    Ok(ReconcileTransition::new(
        new_state,
        "dexie_offer_not_found",
        "dexie_get_offer_404",
        Some(signal),
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
