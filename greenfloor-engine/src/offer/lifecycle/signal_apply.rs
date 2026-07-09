//! Shared resolve+persist for watched-offer Coinset signals.

use chrono::Utc;

use crate::cycle::reconcile::{
    resolve_watched_offer_transition_from_signals, CancelSubmittedContext, CoinsetTxSignals,
};
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::cancel_context::chain_confirmed_tx_ids_for_transition;
use super::persist::{persist_offer_lifecycle_transition, ReconcilePersistOptions};

/// Resolve watched-offer transition from signals and persist when state changes.
///
/// Merges `confirmed_tx_ids` with the tracked cancel tx via
/// [`chain_confirmed_tx_ids_for_transition`] so WS and Dexie share one promotion path.
///
/// # Errors
///
/// Returns an error if reconcile or `SQLite` persist fails.
#[allow(clippy::too_many_arguments)]
pub fn apply_watched_offer_signals(
    store: &SqliteStore,
    market_id: &str,
    offer_id: &str,
    current_state: &str,
    status: Option<i64>,
    signals: CoinsetTxSignals,
    cancel_submitted: Option<&CancelSubmittedContext>,
    options: &ReconcilePersistOptions<'_>,
) -> SignerResult<()> {
    let chain_confirmed =
        chain_confirmed_tx_ids_for_transition(store, cancel_submitted, &signals.confirmed_tx_ids)?;
    let transition = resolve_watched_offer_transition_from_signals(
        current_state,
        status,
        signals,
        &chain_confirmed,
        cancel_submitted,
        Utc::now(),
    )
    .map_err(|err| crate::error::SignerError::Other(err.to_string()))?;
    if !transition.changed {
        return Ok(());
    }
    persist_offer_lifecycle_transition(store, market_id, offer_id, &transition, None, options)
}
