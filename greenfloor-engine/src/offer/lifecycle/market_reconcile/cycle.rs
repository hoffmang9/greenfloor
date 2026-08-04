//! Per-market reconcile: Dexie list fetch and lifecycle transitions.

use std::collections::HashMap;

use chrono::Utc;
use serde_json::json;
use tracing::Level;

use crate::adapters::DexieClient;
use crate::config::{resolve_quote_asset_for_offer, resolve_trade_asset_for_network, MarketConfig};
use crate::error::SignerResult;
use crate::operator_log::{LogContext, DEXIE_OFFERS_ERROR, DEXIE_WATCHLIST_AUGMENT_ERROR};
use crate::storage::SqliteStore;

use super::super::dexie_index::{build_dexie_size_by_offer_id, dexie_status_index};
use super::super::reconcile_prep::{fetch_and_ensure_watches, prepare_market_reconcile_local};
use super::super::{apply_cancel_submitted_rows, ReconcilePersistOptions};
use super::augment::augment_dexie_offers_for_watchlist;
use super::transition::{apply_dexie_lifecycle_transitions, ReconcileMarketCycleMetrics};

#[derive(Debug, Clone)]
pub struct ReconcileMarketCycleResult {
    pub dexie_size_by_offer_id: HashMap<String, i64>,
    /// Dexie status keyed by every list lookup key (`trade_id` ∪ bech32 `id`).
    /// Built once in reconcile; cancel consumes this map (no re-walk of JSON).
    pub dexie_status_by_lookup_key: HashMap<String, i64>,
    pub dexie_fetch_error: Option<String>,
    pub metrics: ReconcileMarketCycleMetrics,
}

impl ReconcileMarketCycleResult {
    fn idle(metrics: ReconcileMarketCycleMetrics) -> Self {
        Self {
            dexie_size_by_offer_id: HashMap::default(),
            dexie_status_by_lookup_key: HashMap::default(),
            dexie_fetch_error: None,
            metrics,
        }
    }

    fn fetch_failed(metrics: ReconcileMarketCycleMetrics, error: String) -> Self {
        Self {
            dexie_fetch_error: Some(error),
            ..Self::idle(metrics)
        }
    }
}

/// Run one market's Dexie list fetch + lifecycle apply for the daemon cycle.
///
/// # Errors
///
/// Returns an error if local prepare, Dexie augment, or lifecycle persist fails.
pub async fn run_reconcile_market_cycle(
    store: &SqliteStore,
    dexie: &DexieClient,
    market: &MarketConfig,
    network: &str,
) -> SignerResult<ReconcileMarketCycleResult> {
    let market_id = market.market_id.as_str();
    let mut metrics = ReconcileMarketCycleMetrics::default();

    // One scan: cancel-submitted rows, local metadata heal, Dexie roles, state map.
    let local = prepare_market_reconcile_local(store, market_id)?;
    apply_cancel_submitted_rows(
        store,
        &local.cancel_submitted_rows,
        &ReconcilePersistOptions {
            action: "cancel_submitted_orphan_reconcile",
            venue: None,
            dexie_error: None,
        },
        Utc::now(),
    )?;
    if !local.dexie.needs_dexie_http() {
        return Ok(ReconcileMarketCycleResult::idle(metrics));
    }
    let plan = local.dexie;
    let mut state_by_offer_id = local.state_by_offer_id;

    let list_offers = match dexie
        .get_offers(
            &resolve_trade_asset_for_network(&market.base_asset, network),
            &resolve_quote_asset_for_offer(&market.quote_asset, network),
        )
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            metrics.cycle_errors += 1;
            LogContext::MARKET_CYCLE.dual_audit(
                store,
                Level::WARN,
                "dexie offers fetch failed",
                DEXIE_OFFERS_ERROR,
                &json!({"market_id": market_id, "error": err.to_string()}),
                Some(market_id),
            )?;
            return Ok(ReconcileMarketCycleResult::fetch_failed(
                metrics,
                err.to_string(),
            ));
        }
    };

    // Heal-only: Dexie payloads → watches. No lifecycle.
    fetch_and_ensure_watches(
        dexie,
        store,
        market_id,
        &plan.heal_only,
        &list_offers,
        &mut |market_id, offer_id, err| {
            metrics.cycle_errors += 1;
            LogContext::MARKET_CYCLE.dual_audit(
                store,
                Level::WARN,
                "dexie watch heal fetch failed",
                DEXIE_WATCHLIST_AUGMENT_ERROR,
                &json!({
                    "market_id": market_id,
                    "offer_id": offer_id,
                    "error": err,
                }),
                Some(market_id),
            )
        },
    )
    .await?;

    if plan.authoritative.is_empty() {
        return Ok(ReconcileMarketCycleResult::idle(metrics));
    }

    let augmented = augment_dexie_offers_for_watchlist(
        dexie,
        store,
        market_id,
        &list_offers,
        &plan.authoritative,
        &mut state_by_offer_id,
        &mut metrics,
    )
    .await?;
    apply_dexie_lifecycle_transitions(
        store,
        market_id,
        &augmented.by_local_id,
        &mut state_by_offer_id,
        &mut metrics,
    )?;
    let authoritative_offers: Vec<_> = augmented.by_local_id.into_values().collect();

    Ok(ReconcileMarketCycleResult {
        dexie_size_by_offer_id: build_dexie_size_by_offer_id(
            &authoritative_offers,
            &market.base_asset,
        ),
        dexie_status_by_lookup_key: dexie_status_index(&authoritative_offers),
        dexie_fetch_error: None,
        metrics,
    })
}
