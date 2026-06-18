use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::adapters::DexieClient;
use crate::cycle::{is_dexie_offer_missing_error_text, resolve_missing_watched_offer_transition};
use crate::error::{SignerError, SignerResult};
use crate::storage::SqliteStore;

use super::reconcile_market_cycle::{
    apply_reconcile_transition, ReconcileMarketCycleMetrics, ReconcileTransitionParams,
};
use crate::offer::lifecycle::missing_offer_error_from_payload;

pub struct AugmentedDexieOffers {
    pub offers: Vec<Value>,
}

fn offer_id_from_payload(offer: &Value) -> Option<String> {
    offer
        .as_object()
        .and_then(|obj| obj.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn apply_missing_watched_offer(
    store: &SqliteStore,
    market_id: &str,
    watched_offer_id: &str,
    error_text: &str,
    state_by_offer_id: &mut HashMap<String, String>,
    metrics: &mut ReconcileMarketCycleMetrics,
) -> SignerResult<()> {
    let current_state = state_by_offer_id
        .get(watched_offer_id)
        .map(String::as_str)
        .unwrap_or("open");
    let transition = resolve_missing_watched_offer_transition(current_state)
        .map_err(|parse_err| SignerError::Other(parse_err.to_string()))?;
    apply_reconcile_transition(ReconcileTransitionParams {
        store,
        market_id,
        offer_id: watched_offer_id,
        transition: &transition,
        metrics,
        state_by_offer_id,
        last_seen_status: None,
        dexie_error: Some(error_text),
    })
}

pub async fn augment_dexie_offers_for_watchlist(
    dexie: &DexieClient,
    store: &SqliteStore,
    market_id: &str,
    list_offers: &[Value],
    our_offer_ids: &HashSet<String>,
    state_by_offer_id: &mut HashMap<String, String>,
    metrics: &mut ReconcileMarketCycleMetrics,
) -> SignerResult<AugmentedDexieOffers> {
    let dexie_offer_ids_in_list: HashSet<String> = list_offers
        .iter()
        .filter_map(offer_id_from_payload)
        .collect();

    let mut augmented_by_id: HashMap<String, Value> = HashMap::new();
    for offer in list_offers {
        if let Some(offer_id) = offer_id_from_payload(offer) {
            augmented_by_id.insert(offer_id, offer.clone());
        }
    }

    let beyond_cap_ids: HashSet<String> = our_offer_ids
        .difference(&dexie_offer_ids_in_list)
        .cloned()
        .collect();
    let mut missing_watched_offer_ids = HashSet::new();

    for watched_offer_id in our_offer_ids.iter() {
        if augmented_by_id.contains_key(watched_offer_id) {
            continue;
        }
        match dexie.get_offer(watched_offer_id).await {
            Ok(payload) => {
                if let Some(error_text) = missing_offer_error_from_payload(&payload) {
                    missing_watched_offer_ids.insert(watched_offer_id.clone());
                    apply_missing_watched_offer(
                        store,
                        market_id,
                        watched_offer_id,
                        &error_text,
                        state_by_offer_id,
                        metrics,
                    )?;
                } else if let Some(single_offer) = payload.get("offer") {
                    augmented_by_id.insert(watched_offer_id.clone(), single_offer.clone());
                }
            }
            Err(err) if is_dexie_offer_missing_error_text(&err.to_string()) => {
                missing_watched_offer_ids.insert(watched_offer_id.clone());
                apply_missing_watched_offer(
                    store,
                    market_id,
                    watched_offer_id,
                    &err.to_string(),
                    state_by_offer_id,
                    metrics,
                )?;
            }
            Err(err) => {
                metrics.cycle_errors += 1;
                store.add_audit_event(
                    "dexie_watchlist_augment_error",
                    &serde_json::json!({
                        "market_id": market_id,
                        "offer_id": watched_offer_id,
                        "error": err.to_string(),
                    }),
                    Some(market_id),
                )?;
            }
        }
    }

    for beyond_offer_id in beyond_cap_ids.difference(&missing_watched_offer_ids) {
        if augmented_by_id.contains_key(beyond_offer_id) {
            continue;
        }
        match dexie.get_offer(beyond_offer_id).await {
            Ok(payload) => {
                if let Some(single_offer) = payload.get("offer") {
                    augmented_by_id.insert(beyond_offer_id.clone(), single_offer.clone());
                }
            }
            Err(err) => {
                metrics.cycle_errors += 1;
                store.add_audit_event(
                    "dexie_watchlist_augment_error",
                    &serde_json::json!({
                        "market_id": market_id,
                        "offer_id": beyond_offer_id,
                        "error": err.to_string(),
                    }),
                    Some(market_id),
                )?;
            }
        }
    }

    Ok(AugmentedDexieOffers {
        offers: augmented_by_id.into_values().collect(),
    })
}

pub fn merge_reconcile_immediate_requeue(
    state: &mut crate::cycle::MarketCycleResultState,
    metrics: &ReconcileMarketCycleMetrics,
) {
    if !metrics.immediate_requeue_requested {
        return;
    }
    for signal in &metrics.immediate_requeue_signals {
        state.request_immediate_requeue(Some(signal.clone()));
    }
    if metrics.immediate_requeue_signals.is_empty() {
        state.request_immediate_requeue(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cycle::MarketCycleResultState;

    #[test]
    fn merge_reconcile_immediate_requeue_populates_cycle_state() {
        let mut state = MarketCycleResultState::default();
        let metrics = ReconcileMarketCycleMetrics {
            immediate_requeue_requested: true,
            immediate_requeue_signals: vec!["taker_fill".to_string()],
            ..ReconcileMarketCycleMetrics::default()
        };
        merge_reconcile_immediate_requeue(&mut state, &metrics);
        assert!(state.immediate_requeue_requested);
        assert_eq!(
            state.immediate_requeue_signals,
            vec!["taker_fill".to_string()]
        );
    }

    #[test]
    fn merge_reconcile_immediate_requeue_without_signal_still_flags() {
        let mut state = MarketCycleResultState::default();
        let metrics = ReconcileMarketCycleMetrics {
            immediate_requeue_requested: true,
            ..ReconcileMarketCycleMetrics::default()
        };
        merge_reconcile_immediate_requeue(&mut state, &metrics);
        assert!(state.immediate_requeue_requested);
        assert!(state.immediate_requeue_signals.is_empty());
    }
}
