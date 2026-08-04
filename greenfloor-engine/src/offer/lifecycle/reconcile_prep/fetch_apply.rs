//! Fetch a watched Dexie offer and persist the resolved lifecycle transition.

use crate::adapters::DexieClient;
use crate::cycle::CycleOfferTransition;
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::super::persist::ReconcilePersistOptions;
use super::super::signal_apply::persist_resolved_watched_transition;
use super::super::transition::{
    resolve_watched_offer_transition_from_dexie_fetch, WatchedOfferTransitionEnv,
};

/// Resolve a watched offer via Dexie `get_offer` and persist the transition.
///
/// Composes [`resolve_watched_offer_transition_from_dexie_fetch`] with the shared
/// persist helpers used by CLI batch reconcile and daemon augment.
///
/// # Errors
///
/// Returns an error if Dexie lookup, transition resolve, or `SQLite` persist fails.
pub async fn fetch_and_apply_watched_offer(
    store: &SqliteStore,
    dexie: &DexieClient,
    market_id: &str,
    offer_id: &str,
    current_state: &str,
    env: WatchedOfferTransitionEnv<'_>,
    options: &ReconcilePersistOptions<'_>,
) -> SignerResult<(CycleOfferTransition, Option<i64>)> {
    let (transition, status, dexie_error) = resolve_watched_offer_transition_from_dexie_fetch(
        store,
        dexie,
        offer_id,
        current_state,
        env,
    )
    .await?;
    let persist_options = if let Some(error_text) = dexie_error.as_deref() {
        ReconcilePersistOptions {
            action: options.action,
            venue: options.venue,
            dexie_error: Some(error_text),
        }
    } else {
        ReconcilePersistOptions {
            action: options.action,
            venue: options.venue,
            dexie_error: options.dexie_error,
        }
    };
    persist_resolved_watched_transition(
        store,
        market_id,
        offer_id,
        &transition,
        status,
        &persist_options,
    )?;
    Ok((transition, status))
}
