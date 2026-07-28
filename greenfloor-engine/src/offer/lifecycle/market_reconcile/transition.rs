//! Dexie reconcile transition apply + apply-outcome metrics.

use std::collections::HashMap;

use serde_json::Value;

use crate::cycle::CycleOfferTransition;
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::super::{
    apply_watched_offer_from_dexie_payload, preload_cancel_submitted_contexts,
    ReconcilePersistOptions, WatchedOfferTransitionEnv,
};

/// Outcomes from market reconcile apply (errors + requeue hints for the daemon cycle).
#[derive(Debug, Clone, Default)]
pub struct ReconcileMarketCycleMetrics {
    pub cycle_errors: u64,
    pub immediate_requeue_requested: bool,
    pub immediate_requeue_signals: Vec<String>,
}

/// Update cycle metrics / in-memory state after a lifecycle transition was applied.
pub(crate) fn note_reconcile_transition_side_effects(
    transition: &CycleOfferTransition,
    offer_id: &str,
    metrics: &mut ReconcileMarketCycleMetrics,
    state_by_offer_id: &mut HashMap<String, String>,
) {
    if transition.changed {
        state_by_offer_id.insert(
            offer_id.to_string(),
            transition.new_state.as_str().into_owned(),
        );
    }
    if transition.immediate_requeue {
        metrics.immediate_requeue_requested = true;
        if let Some(signal) = transition.signal {
            metrics
                .immediate_requeue_signals
                .push(signal.as_str().to_string());
        }
    }
}

/// Apply Dexie-authoritative lifecycle transitions for payloads keyed by local offer id.
pub(crate) fn apply_dexie_lifecycle_transitions(
    store: &SqliteStore,
    market_id: &str,
    by_local_id: &HashMap<String, Value>,
    state_by_offer_id: &mut HashMap<String, String>,
    metrics: &mut ReconcileMarketCycleMetrics,
) -> SignerResult<()> {
    if by_local_id.is_empty() {
        return Ok(());
    }
    let offer_ids: Vec<String> = by_local_id.keys().cloned().collect();
    let offer_rows = store.list_offer_states_for_ids(&offer_ids)?;
    let cancel_submitted_by_offer = preload_cancel_submitted_contexts(store, &offer_rows)?;
    let options = ReconcilePersistOptions {
        action: "reconcile_coins_and_offers",
        venue: Some(crate::config::Venue::Dexie),
        dexie_error: None,
    };
    let env = WatchedOfferTransitionEnv::at_now(Some(&cancel_submitted_by_offer));

    for (local_offer_id, raw) in by_local_id {
        let current_state = state_by_offer_id
            .get(local_offer_id)
            .map_or("open", String::as_str);
        let (transition, _) = apply_watched_offer_from_dexie_payload(
            store,
            market_id,
            local_offer_id,
            current_state,
            raw,
            env,
            &options,
        )?;
        note_reconcile_transition_side_effects(
            &transition,
            local_offer_id,
            metrics,
            state_by_offer_id,
        );
    }
    Ok(())
}
