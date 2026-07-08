//! Shared resolve+persist for watched-offer Coinset signals.

use chrono::Utc;

use crate::cycle::reconcile::{
    resolve_watched_offer_transition_with_summary, CancelSubmittedContext, CoinsetSignalSummary,
    CoinsetTxSignals,
};
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::persist::{persist_offer_lifecycle_transition, ReconcilePersistOptions};

/// Resolve watched-offer transition from signals and persist when state changes.
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
    summary_override: Option<CoinsetSignalSummary>,
    cancel_submitted: Option<&CancelSubmittedContext>,
    options: &ReconcilePersistOptions<'_>,
) -> SignerResult<()> {
    let summary = summary_override.unwrap_or_else(|| signals.summary());
    let transition = resolve_watched_offer_transition_with_summary(
        current_state,
        status,
        summary,
        signals,
        &[],
        cancel_submitted,
        Utc::now(),
    )
    .map_err(|err| crate::error::SignerError::Other(err.to_string()))?;
    if !transition.changed {
        return Ok(());
    }
    persist_offer_lifecycle_transition(store, market_id, offer_id, &transition, None, options)
}
