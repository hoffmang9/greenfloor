//! Augment Dexie list results for Dexie-authoritative watched offers only.

use std::collections::{HashMap, HashSet};

use serde_json::Value;
use tracing::Level;

use crate::adapters::DexieClient;
use crate::cycle::{is_dexie_offer_missing_error_text, resolve_missing_watched_offer_transition};
use crate::error::{SignerError, SignerResult};
use crate::operator_log::{LogContext, DEXIE_WATCHLIST_AUGMENT_ERROR};
use crate::storage::SqliteStore;

use super::dexie_size::index_list_offers_by_local_ids;
use super::reconcile_transition::{
    note_reconcile_transition_side_effects, ReconcileMarketCycleMetrics,
};
use super::watch_plan::ensure_watches_from_dexie_payload;
use crate::offer::lifecycle::{
    missing_offer_error_from_payload, persist_resolved_watched_transition, ReconcilePersistOptions,
};

pub struct AugmentedDexieOffers {
    /// Dexie payloads keyed by local `offer_state.offer_id` (already matched).
    pub by_local_id: HashMap<String, Value>,
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
        .map_or("open", String::as_str);
    let transition = resolve_missing_watched_offer_transition(current_state)
        .map_err(|parse_err| SignerError::Other(parse_err.to_string()))?;
    persist_resolved_watched_transition(
        store,
        market_id,
        watched_offer_id,
        &transition,
        None,
        &ReconcilePersistOptions {
            action: "reconcile_coins_and_offers",
            venue: Some(crate::config::Venue::Dexie),
            dexie_error: Some(error_text),
        },
    )?;
    note_reconcile_transition_side_effects(
        &transition,
        watched_offer_id,
        metrics,
        state_by_offer_id,
    );
    Ok(())
}

fn record_watchlist_augment_error(
    store: &SqliteStore,
    market_id: &str,
    offer_id: &str,
    error: &str,
    metrics: &mut ReconcileMarketCycleMetrics,
) -> SignerResult<()> {
    metrics.cycle_errors += 1;
    LogContext::MARKET_CYCLE.dual_audit(
        store,
        Level::WARN,
        "dexie watchlist augment failed",
        DEXIE_WATCHLIST_AUGMENT_ERROR,
        &serde_json::json!({
            "market_id": market_id,
            "offer_id": offer_id,
            "error": error,
        }),
        Some(market_id),
    )
}

async fn fetch_missing_watched_offers(
    dexie: &DexieClient,
    store: &SqliteStore,
    market_id: &str,
    dexie_offer_ids: &HashSet<String>,
    augmented_by_local_id: &mut HashMap<String, Value>,
    state_by_offer_id: &mut HashMap<String, String>,
    metrics: &mut ReconcileMarketCycleMetrics,
) -> SignerResult<()> {
    for watched_offer_id in dexie_offer_ids {
        if augmented_by_local_id.contains_key(watched_offer_id) {
            continue;
        }
        match dexie.get_offer(watched_offer_id).await {
            Ok(response) => {
                let payload = response.body();
                if let Some(error_text) = missing_offer_error_from_payload(payload) {
                    apply_missing_watched_offer(
                        store,
                        market_id,
                        watched_offer_id,
                        &error_text,
                        state_by_offer_id,
                        metrics,
                    )?;
                } else if let Some(single_offer) = payload.get("offer") {
                    if crate::daemon::dexie_size::offer_matches_local_id(
                        single_offer,
                        watched_offer_id,
                    ) {
                        augmented_by_local_id
                            .insert(watched_offer_id.clone(), single_offer.clone());
                    } else {
                        record_watchlist_augment_error(
                            store,
                            market_id,
                            watched_offer_id,
                            "dexie get_offer payload did not match local offer id",
                            metrics,
                        )?;
                    }
                }
            }
            Err(err) if is_dexie_offer_missing_error_text(&err.to_string()) => {
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
                record_watchlist_augment_error(
                    store,
                    market_id,
                    watched_offer_id,
                    &err.to_string(),
                    metrics,
                )?;
            }
        }
    }
    Ok(())
}

/// Augment Dexie list results for Dexie-authoritative watched offers only.
///
/// Callers must pass the Dexie-authoritative subset. Their payloads also heal any
/// missing durable watches. Heal-only NULL-venue rows use
/// [`super::watch_plan::fetch_and_ensure_watches`] without lifecycle authority.
pub async fn augment_dexie_offers_for_watchlist(
    dexie: &DexieClient,
    store: &SqliteStore,
    market_id: &str,
    list_offers: &[Value],
    dexie_offer_ids: &HashSet<String>,
    state_by_offer_id: &mut HashMap<String, String>,
    metrics: &mut ReconcileMarketCycleMetrics,
) -> SignerResult<AugmentedDexieOffers> {
    if dexie_offer_ids.is_empty() {
        return Ok(AugmentedDexieOffers {
            by_local_id: HashMap::default(),
        });
    }
    let mut augmented_by_local_id = index_list_offers_by_local_ids(list_offers, dexie_offer_ids);
    fetch_missing_watched_offers(
        dexie,
        store,
        market_id,
        dexie_offer_ids,
        &mut augmented_by_local_id,
        state_by_offer_id,
        metrics,
    )
    .await?;
    for (offer_id, payload) in &augmented_by_local_id {
        ensure_watches_from_dexie_payload(store, market_id, offer_id, payload)?;
    }

    Ok(AugmentedDexieOffers {
        by_local_id: augmented_by_local_id,
    })
}
