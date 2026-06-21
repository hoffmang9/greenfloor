use std::borrow::Cow;

use crate::cycle::lifecycle::{apply_offer_signal, OfferLifecycleState, OfferSignal};

use super::decision::resolve_watched_offer_decision;
use super::state::{ReconcileState, ReconcileStateError, TAKER_NONE};
use super::transition::{
    build_transition, build_transition_from_decision, empty_coinset_lists, CycleOfferTransition,
    TransitionBuildArgs,
};

/// Unchanged offer transition.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn unchanged_offer_transition(
    current_state: &str,
    reason: impl Into<String>,
) -> Result<CycleOfferTransition, ReconcileStateError> {
    let (coinset_tx_ids, coinset_confirmed_tx_ids, coinset_mempool_tx_ids) = empty_coinset_lists();
    Ok(build_transition(TransitionBuildArgs {
        current_state,
        new_state: ReconcileState::parse(current_state)?,
        reason: Cow::Owned(reason.into()),
        signal_source: "none",
        signal: None,
        immediate_requeue: false,
        taker_signal: TAKER_NONE,
        taker_diagnostic: TAKER_NONE,
        coinset_tx_ids,
        coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids,
    }))
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
    ReconcileState::parse(current_state)?;
    let (coinset_tx_ids, coinset_confirmed_tx_ids, coinset_mempool_tx_ids) = empty_coinset_lists();
    Ok(build_transition(TransitionBuildArgs {
        current_state,
        new_state: ReconcileState::UnsupportedVenue,
        reason: Cow::Owned(format!("unsupported_venue:{venue}")),
        signal_source: "none",
        signal: None,
        immediate_requeue: false,
        taker_signal: TAKER_NONE,
        taker_diagnostic: TAKER_NONE,
        coinset_tx_ids,
        coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids,
    }))
}

/// Resolve missing watched offer transition.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn resolve_missing_watched_offer_transition(
    current_state: &str,
) -> Result<CycleOfferTransition, ReconcileStateError> {
    let parsed = ReconcileState::parse(current_state)?;
    let (coinset_tx_ids, coinset_confirmed_tx_ids, coinset_mempool_tx_ids) = empty_coinset_lists();
    if parsed.is_terminal() {
        return Ok(build_transition(TransitionBuildArgs {
            current_state,
            new_state: parsed,
            reason: Cow::Borrowed("dexie_offer_not_found_preserved_terminal"),
            signal_source: "dexie_get_offer_404",
            signal: None,
            immediate_requeue: false,
            taker_signal: TAKER_NONE,
            taker_diagnostic: TAKER_NONE,
            coinset_tx_ids,
            coinset_confirmed_tx_ids,
            coinset_mempool_tx_ids,
        }));
    }
    let transition = apply_offer_signal(OfferLifecycleState::Open, OfferSignal::Expired);
    Ok(build_transition(TransitionBuildArgs {
        current_state,
        new_state: ReconcileState::Lifecycle(transition.new_state),
        reason: Cow::Borrowed("dexie_offer_not_found"),
        signal_source: "dexie_get_offer_404",
        signal: Some(OfferSignal::Expired),
        immediate_requeue: true,
        taker_signal: TAKER_NONE,
        taker_diagnostic: TAKER_NONE,
        coinset_tx_ids,
        coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids,
    }))
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
    let parsed = ReconcileState::parse(current_state)?;
    let decision = resolve_watched_offer_decision(
        &parsed,
        status,
        &coinset_tx_ids,
        &coinset_confirmed_tx_ids,
        &coinset_mempool_tx_ids,
    );
    Ok(build_transition_from_decision(
        current_state,
        decision,
        coinset_tx_ids,
        coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids,
    ))
}
