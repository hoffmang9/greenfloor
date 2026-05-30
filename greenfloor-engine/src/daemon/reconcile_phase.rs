use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use crate::adapters::DexieClient;
use crate::config::{resolve_quote_asset_for_offer, resolve_trade_asset_for_network, MarketConfig};
use crate::cycle::{
    is_dexie_offer_missing_error_text, resolve_missing_watched_offer_transition,
    resolve_watched_offer_transition_from_signals, CycleOfferTransition,
};
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::coinset_tx::{
    build_dexie_size_by_offer_id, dexie_offer_status, extract_coinset_tx_ids_from_offer_payload,
};
use super::watchlist::{update_market_coin_watchlist_from_offers, watchlist_offer_ids};

#[derive(Debug, Clone, Default)]
pub struct ReconcilePhaseMetrics {
    pub cycle_errors: u64,
    pub immediate_requeue_requested: bool,
    pub immediate_requeue_signals: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ReconcilePhaseResult {
    pub offers: Vec<Value>,
    pub dexie_size_by_offer_id: HashMap<String, i64>,
    pub dexie_fetch_error: Option<String>,
    pub metrics: ReconcilePhaseMetrics,
}

fn coinset_signal_lists(
    store: &SqliteStore,
    coinset_tx_ids: &[String],
) -> SignerResult<(Vec<String>, Vec<String>)> {
    if coinset_tx_ids.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let signal_by_tx_id = store.get_tx_signal_state(coinset_tx_ids)?;
    let mut confirmed = Vec::new();
    let mut mempool = Vec::new();
    for tx_id in coinset_tx_ids {
        let Some(signal) = signal_by_tx_id.get(tx_id) else {
            continue;
        };
        if signal.tx_block_confirmed_at.is_some() {
            confirmed.push(tx_id.clone());
            continue;
        }
        if signal.mempool_observed_at.is_some() {
            mempool.push(tx_id.clone());
        }
    }
    Ok((confirmed, mempool))
}

fn persist_offer_lifecycle_transition(
    store: &SqliteStore,
    market_id: &str,
    offer_id: &str,
    transition: &CycleOfferTransition,
    last_seen_status: Option<i64>,
    dexie_error: Option<&str>,
) -> SignerResult<()> {
    store.upsert_offer_state(offer_id, market_id, &transition.new_state, last_seen_status)?;
    let mut payload = json!({
        "offer_id": offer_id,
        "market_id": market_id,
        "old_state": transition.old_state,
        "new_state": transition.new_state,
        "changed": transition.changed,
        "reason": transition.reason,
        "signal": transition.signal,
        "signal_source": transition.signal_source,
        "last_seen_status": last_seen_status,
        "dexie_status": last_seen_status,
        "coinset_tx_ids": transition.coinset_tx_ids,
        "coinset_confirmed_tx_ids": transition.coinset_confirmed_tx_ids,
        "coinset_mempool_tx_ids": transition.coinset_mempool_tx_ids,
        "taker_signal": transition.taker_signal,
        "taker_diagnostic": transition.taker_diagnostic,
        "action": "reconcile_coins_and_offers",
    });
    if let Some(error) = dexie_error {
        if let Value::Object(obj) = &mut payload {
            obj.insert("dexie_error".to_string(), Value::String(error.to_string()));
        }
    }
    store.add_audit_event("offer_lifecycle_transition", &payload, Some(market_id))?;
    if transition.taker_signal != "none" {
        store.add_audit_event(
            "taker_detection",
            &json!({
                "offer_id": offer_id,
                "market_id": market_id,
                "venue": "dexie",
                "signal": transition.taker_signal,
                "advisory_diagnostic": transition.taker_diagnostic,
                "old_state": transition.old_state,
                "new_state": transition.new_state,
                "last_seen_status": last_seen_status,
                "signal_source": transition.signal_source,
                "coinset_confirmed_tx_ids": transition.coinset_confirmed_tx_ids,
            }),
            Some(market_id),
        )?;
    }
    Ok(())
}

fn apply_reconcile_transition(
    store: &SqliteStore,
    market_id: &str,
    offer_id: &str,
    transition: &CycleOfferTransition,
    metrics: &mut ReconcilePhaseMetrics,
    state_by_offer_id: &mut HashMap<String, String>,
    last_seen_status: Option<i64>,
    dexie_error: Option<&str>,
) -> SignerResult<()> {
    persist_offer_lifecycle_transition(
        store,
        market_id,
        offer_id,
        transition,
        last_seen_status,
        dexie_error,
    )?;
    if transition.changed {
        state_by_offer_id.insert(offer_id.to_string(), transition.new_state.clone());
    }
    if transition.immediate_requeue {
        metrics.immediate_requeue_requested = true;
        if let Some(signal) = transition.signal.as_deref() {
            metrics.immediate_requeue_signals.push(signal.to_string());
        }
    }
    Ok(())
}

fn transition_from_dexie_offer_payload(
    store: &SqliteStore,
    current_state: &str,
    offer_payload: &Value,
) -> SignerResult<CycleOfferTransition> {
    let status = dexie_offer_status(offer_payload);
    let coinset_tx_ids = extract_coinset_tx_ids_from_offer_payload(offer_payload);
    let (coinset_confirmed_tx_ids, coinset_mempool_tx_ids) =
        coinset_signal_lists(store, &coinset_tx_ids)?;
    resolve_watched_offer_transition_from_signals(
        current_state,
        status,
        coinset_tx_ids,
        coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids,
    )
    .map_err(|err| crate::error::SignerError::Other(err.to_string()))
}

pub async fn run_market_reconcile_phase(
    store: &SqliteStore,
    dexie: &DexieClient,
    market: &MarketConfig,
    network: &str,
) -> SignerResult<ReconcilePhaseResult> {
    let market_id = market.market_id.as_str();
    let mut metrics = ReconcilePhaseMetrics::default();
    let dexie_offered_asset = resolve_trade_asset_for_network(&market.base_asset, network);
    let dexie_requested_asset = resolve_quote_asset_for_offer(&market.quote_asset, network);

    let offers = match dexie
        .get_offers(&dexie_offered_asset, &dexie_requested_asset)
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            metrics.cycle_errors += 1;
            store.add_audit_event(
                "dexie_offers_error",
                &json!({"market_id": market_id, "error": err.to_string()}),
                Some(market_id),
            )?;
            return Ok(ReconcilePhaseResult {
                offers: Vec::new(),
                dexie_size_by_offer_id: HashMap::new(),
                dexie_fetch_error: Some(err.to_string()),
                metrics,
            });
        }
    };

    let our_offer_ids = watchlist_offer_ids(store, market_id)?;
    let mut state_by_offer_id: HashMap<String, String> = store
        .list_offer_state_details(market_id, 5000)?
        .into_iter()
        .map(|row| (row.offer_id, row.state))
        .collect();

    let dexie_offer_ids_in_list: HashSet<String> = offers
        .iter()
        .filter_map(|offer| {
            offer
                .as_object()
                .and_then(|obj| obj.get("id"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .collect();

    let mut augmented_by_id: HashMap<String, Value> = HashMap::new();
    for offer in &offers {
        if let Some(offer_id) = offer
            .as_object()
            .and_then(|obj| obj.get("id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            augmented_by_id.insert(offer_id.to_string(), offer.clone());
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
                if let Some(single_offer) = payload.get("offer") {
                    augmented_by_id.insert(watched_offer_id.clone(), single_offer.clone());
                }
            }
            Err(err) if is_dexie_offer_missing_error_text(&err.to_string()) => {
                missing_watched_offer_ids.insert(watched_offer_id.clone());
                let current_state = state_by_offer_id
                    .get(watched_offer_id)
                    .map(String::as_str)
                    .unwrap_or("open");
                let transition = resolve_missing_watched_offer_transition(current_state)
                    .map_err(|parse_err| crate::error::SignerError::Other(parse_err.to_string()))?;
                apply_reconcile_transition(
                    store,
                    market_id,
                    watched_offer_id,
                    &transition,
                    &mut metrics,
                    &mut state_by_offer_id,
                    None,
                    Some(&err.to_string()),
                )?;
            }
            Err(_) => {}
        }
    }

    for beyond_offer_id in beyond_cap_ids.difference(&missing_watched_offer_ids) {
        if augmented_by_id.contains_key(beyond_offer_id) {
            continue;
        }
        if let Ok(payload) = dexie.get_offer(beyond_offer_id).await {
            if let Some(single_offer) = payload.get("offer") {
                augmented_by_id.insert(beyond_offer_id.clone(), single_offer.clone());
            }
        }
    }

    let augmented_offers: Vec<Value> = augmented_by_id.into_values().collect();
    let dexie_size_by_offer_id =
        build_dexie_size_by_offer_id(&augmented_offers, &market.base_asset);

    for offer in &augmented_offers {
        let offer_id = offer
            .as_object()
            .and_then(|obj| obj.get("id"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if offer_id.is_empty() || !our_offer_ids.contains(&offer_id) {
            continue;
        }
        let status = dexie_offer_status(offer);
        let current_state = state_by_offer_id
            .get(&offer_id)
            .map(String::as_str)
            .unwrap_or("open");
        let transition = transition_from_dexie_offer_payload(store, current_state, offer)?;
        apply_reconcile_transition(
            store,
            market_id,
            &offer_id,
            &transition,
            &mut metrics,
            &mut state_by_offer_id,
            status,
            None,
        )?;
    }

    update_market_coin_watchlist_from_offers(store, market_id, &augmented_offers)?;

    Ok(ReconcilePhaseResult {
        offers: augmented_offers,
        dexie_size_by_offer_id,
        dexie_fetch_error: None,
        metrics,
    })
}
