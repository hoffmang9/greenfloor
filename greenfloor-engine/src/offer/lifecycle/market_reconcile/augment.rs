//! Augment Dexie list results for Dexie-authoritative watched offers only.

use std::collections::{HashMap, HashSet};

use serde_json::Value;
use tracing::Level;

use crate::adapters::DexieClient;
use crate::error::SignerResult;
use crate::operator_log::{LogContext, DEXIE_WATCHLIST_AUGMENT_ERROR};
use crate::storage::SqliteStore;

use super::super::dexie_index::index_list_offers_by_local_ids;
use super::super::reconcile_prep::{
    ensure_watches_from_dexie_payload, fetch_dexie_offer, DexieFetchMode, DexieOfferFetch,
};
use super::super::{persist_missing_watched_offer, ReconcilePersistOptions};
use super::transition::{note_reconcile_transition_side_effects, ReconcileMarketCycleMetrics};

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
    let transition = persist_missing_watched_offer(
        store,
        market_id,
        watched_offer_id,
        current_state,
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

/// Shared Dexie watch-error audit for the daemon cycle heal callback and watchlist augment —
/// same `market_id`/`offer_id`/`error` payload shape and `DEXIE_WATCHLIST_AUGMENT_ERROR`
/// event, differing only in the human-readable `message` for each call site.
pub(super) fn dexie_watch_error_dual_audit(
    store: &SqliteStore,
    message: &'static str,
    market_id: &str,
    offer_id: &str,
    error: &str,
    metrics: &mut ReconcileMarketCycleMetrics,
) -> SignerResult<()> {
    metrics.cycle_errors += 1;
    LogContext::MARKET_CYCLE.dual_audit(
        store,
        Level::WARN,
        message,
        DEXIE_WATCHLIST_AUGMENT_ERROR,
        &serde_json::json!({
            "market_id": market_id,
            "offer_id": offer_id,
            "error": error,
        }),
        Some(market_id),
    )
}

fn record_watchlist_augment_error(
    store: &SqliteStore,
    market_id: &str,
    offer_id: &str,
    error: &str,
    metrics: &mut ReconcileMarketCycleMetrics,
) -> SignerResult<()> {
    dexie_watch_error_dual_audit(
        store,
        "dexie watchlist augment failed",
        market_id,
        offer_id,
        error,
        metrics,
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
        match fetch_dexie_offer(dexie, watched_offer_id, DexieFetchMode::HealStrict).await {
            DexieOfferFetch::Found(body) => {
                augmented_by_local_id.insert(watched_offer_id.clone(), body);
            }
            DexieOfferFetch::Missing(error_text) => {
                apply_missing_watched_offer(
                    store,
                    market_id,
                    watched_offer_id,
                    &error_text,
                    state_by_offer_id,
                    metrics,
                )?;
            }
            DexieOfferFetch::Mismatch => {
                record_watchlist_augment_error(
                    store,
                    market_id,
                    watched_offer_id,
                    "dexie get_offer payload did not match local offer id",
                    metrics,
                )?;
            }
            DexieOfferFetch::LookupError(err) => {
                record_watchlist_augment_error(store, market_id, watched_offer_id, &err, metrics)?;
            }
        }
    }
    Ok(())
}

/// Augment Dexie list results for Dexie-authoritative watched offers only.
///
/// Callers must pass the Dexie-authoritative subset. Their payloads also heal any
/// missing durable watches. Heal-only NULL-venue rows use
/// [`super::super::reconcile_prep::fetch_and_ensure_watches`] without lifecycle authority.
///
/// # Errors
///
/// Returns an error if Dexie HTTP or `SQLite` writes fail.
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
