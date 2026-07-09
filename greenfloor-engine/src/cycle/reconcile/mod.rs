//! Offer transition policy for daemon cycle signals (taker detection, venue rules).
//!
//! Batch DB reconcile lives in `offer::lifecycle::reconcile_watched_offers`; per-market
//! cycle reconcile lives in `daemon::reconcile_market_cycle`.

mod audit_preserve;
mod builders;
mod cancel_submitted_policy;
mod coinset_signals;
mod decision;
mod dispatch;
mod metadata;
mod state;
mod transition;

#[cfg(test)]
mod tests;

use chrono::{DateTime, Utc};

pub use audit_preserve::preserved_lifecycle_transitions;
pub(crate) use audit_preserve::PRESERVED_LIFECYCLE_TRANSITIONS;
pub(crate) use cancel_submitted_policy::allowed_cancel_target_offer_ids;
pub(crate) use cancel_submitted_policy::cancel_tx_chain_confirmed;
pub use cancel_submitted_policy::CancelSubmittedContext;
pub use coinset_signals::{signals_from_ws_offer_status, CoinsetSignalSummary, CoinsetTxSignals};
pub(crate) use metadata::{REASON_POTENTIAL_TAKE_SEEN, REASON_TAKE_CONFIRMED_ON_TX_BLOCK};
pub use state::{ReconcileState, ReconcileStateError};
pub use transition::CycleOfferTransition;

use builders::{
    missing_watched_offer_expired, missing_watched_offer_preserved, unchanged, unsupported_venue,
};
use decision::resolve_watched_offer_decision;

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
    Ok(unchanged(old_state.clone(), reason.into()).into_cycle_transition_no_coinset(old_state))
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
    Ok(unsupported_venue(venue).into_cycle_transition_no_coinset(old_state))
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
    let decision = if old_state.is_terminal() || old_state.is_cancel_submitted() {
        missing_watched_offer_preserved(old_state.clone())
    } else {
        missing_watched_offer_expired()
    };
    Ok(decision.into_cycle_transition_no_coinset(old_state))
}

/// Resolve watched offer transition from signals.
///
/// Dispatch uses `signals.summary()` (including `CoinsetTxSignals::watch_hit()`).
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn resolve_watched_offer_transition_from_signals(
    current_state: &str,
    status: Option<i64>,
    signals: CoinsetTxSignals,
    chain_confirmed_tx_ids: &[String],
    cancel_submitted: Option<&CancelSubmittedContext>,
    now: DateTime<Utc>,
) -> Result<CycleOfferTransition, ReconcileStateError> {
    let old_state = ReconcileState::parse(current_state)?;
    Ok(resolve_watched_offer_decision(
        &old_state,
        status,
        &signals,
        chain_confirmed_tx_ids,
        cancel_submitted,
        now,
    )
    .into_cycle_transition(
        old_state,
        signals.tx_ids,
        signals.confirmed_tx_ids,
        signals.mempool_tx_ids,
    ))
}
