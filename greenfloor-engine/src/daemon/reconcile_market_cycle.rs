//! Per-market daemon-cycle reconcile: Dexie list fetch and lifecycle transitions.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use serde_json::{json, Value};
use tracing::Level;

use crate::adapters::DexieClient;
use crate::config::{resolve_quote_asset_for_offer, resolve_trade_asset_for_network, MarketConfig};
use crate::cycle::CycleOfferTransition;
use crate::error::SignerResult;
use crate::operator_log::{LogContext, DEXIE_OFFERS_ERROR};
use crate::storage::SqliteStore;

use super::dexie_size::build_dexie_size_by_offer_id;
use super::reconcile_augment::augment_dexie_offers_for_watchlist;
use super::watchlist::watchlist_offer_ids;
use crate::offer::lifecycle::{
    persist_offer_lifecycle_transition, preload_cancel_submitted_contexts,
    transition_from_list_offer_payload, ReconcilePersistOptions, WatchedOfferTransitionEnv,
};

#[derive(Debug, Clone, Default)]
pub struct ReconcileMarketCycleMetrics {
    pub cycle_errors: u64,
    pub immediate_requeue_requested: bool,
    pub immediate_requeue_signals: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ReconcileMarketCycleResult {
    pub offers: Vec<Value>,
    pub dexie_size_by_offer_id: HashMap<String, i64>,
    pub dexie_fetch_error: Option<String>,
    pub metrics: ReconcileMarketCycleMetrics,
}

pub(crate) struct ReconcileTransitionParams<'a> {
    pub store: &'a SqliteStore,
    pub market_id: &'a str,
    pub offer_id: &'a str,
    pub transition: &'a CycleOfferTransition,
    pub metrics: &'a mut ReconcileMarketCycleMetrics,
    pub state_by_offer_id: &'a mut HashMap<String, String>,
    pub last_seen_status: Option<i64>,
    pub dexie_error: Option<&'a str>,
}

pub(crate) fn apply_reconcile_transition(
    params: ReconcileTransitionParams<'_>,
) -> SignerResult<()> {
    let ReconcileTransitionParams {
        store,
        market_id,
        offer_id,
        transition,
        metrics,
        state_by_offer_id,
        last_seen_status,
        dexie_error,
    } = params;
    persist_offer_lifecycle_transition(
        store,
        market_id,
        offer_id,
        transition,
        last_seen_status,
        &ReconcilePersistOptions {
            action: "reconcile_coins_and_offers",
            venue: Some("dexie"),
            dexie_error,
        },
    )?;
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
    Ok(())
}

pub async fn run_reconcile_market_cycle(
    store: &SqliteStore,
    dexie: &DexieClient,
    market: &MarketConfig,
    network: &str,
) -> SignerResult<ReconcileMarketCycleResult> {
    let market_id = market.market_id.as_str();
    let mut metrics = ReconcileMarketCycleMetrics::default();
    let dexie_offered_asset = resolve_trade_asset_for_network(&market.base_asset, network);
    let dexie_requested_asset = resolve_quote_asset_for_offer(&market.quote_asset, network);

    let offers = match dexie
        .get_offers(&dexie_offered_asset, &dexie_requested_asset)
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
            return Ok(ReconcileMarketCycleResult {
                offers: Vec::new(),
                dexie_size_by_offer_id: HashMap::default(),
                dexie_fetch_error: Some(err.to_string()),
                metrics,
            });
        }
    };

    let our_offer_ids: HashSet<String> =
        watchlist_offer_ids(store, market_id)?.into_iter().collect();
    let mut state_by_offer_id: HashMap<String, String> = store
        .list_offer_state_details(market_id, 5000)?
        .into_iter()
        .map(|row| (row.offer_id, row.state))
        .collect();

    let augmented = augment_dexie_offers_for_watchlist(
        dexie,
        store,
        market_id,
        &offers,
        &our_offer_ids,
        &mut state_by_offer_id,
        &mut metrics,
    )
    .await?;
    let augmented_offers = augmented.offers;
    let dexie_size_by_offer_id =
        build_dexie_size_by_offer_id(&augmented_offers, &market.base_asset);

    let offer_rows = store.list_offer_states(Some(market_id), 5000)?;
    let cancel_submitted_by_offer = preload_cancel_submitted_contexts(store, &offer_rows)?;
    let env = WatchedOfferTransitionEnv::new(Utc::now(), Some(&cancel_submitted_by_offer));

    for raw in &augmented_offers {
        let Some(offer_id) = raw
            .as_object()
            .and_then(|obj| obj.get("id"))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
        else {
            continue;
        };
        if !our_offer_ids.contains(&offer_id) {
            continue;
        }
        let current_state = state_by_offer_id
            .get(&offer_id)
            .map_or("open", String::as_str);
        let (transition, status) =
            transition_from_list_offer_payload(store, &offer_id, current_state, raw, env)?;
        apply_reconcile_transition(ReconcileTransitionParams {
            store,
            market_id,
            offer_id: &offer_id,
            transition: &transition,
            metrics: &mut metrics,
            state_by_offer_id: &mut state_by_offer_id,
            last_seen_status: status,
            dexie_error: None,
        })?;
    }

    Ok(ReconcileMarketCycleResult {
        offers: augmented_offers,
        dexie_size_by_offer_id,
        dexie_fetch_error: None,
        metrics,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use mockito::Matcher;
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::adapters::DexieClient;
    use crate::config::MarketConfig;
    use crate::storage::SqliteStore;

    fn sample_market(base_asset: &str, quote_asset: &str) -> MarketConfig {
        MarketConfig {
            market_id: "m1".to_string(),
            enabled: true,
            base_asset: base_asset.to_string(),
            base_symbol: "AS1".to_string(),
            quote_asset: quote_asset.to_string(),
            quote_asset_type: "unstable".to_string(),
            receive_address: "xch1test".to_string(),
            signer_key_id: "key-1".to_string(),
            mode: "sell_only".to_string(),
            pricing: json!({}),
            cancel_move_threshold_bps: None,
            ladders: HashMap::default(),
        }
    }

    #[tokio::test]
    async fn reconcile_expires_watched_offer_on_dexie_404() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        store
            .upsert_offer_state("offer-50", "m1", "open", Some(0))
            .expect("seed");

        let mut server = mockito::Server::new_async().await;
        let _list = server
            .mock("GET", Matcher::Regex(r"/v1/offers\?.*".to_string()))
            .with_status(200)
            .with_body(r#"{"success":true,"offers":[]}"#)
            .create();
        let _single = server
            .mock("GET", "/v1/offers/offer-50")
            .with_status(404)
            .with_body(r#"{"success":false,"error":"Not Found"}"#)
            .create();
        let dexie = DexieClient::new(server.url());
        let result =
            run_reconcile_market_cycle(&store, &dexie, &sample_market("asset1", "xch"), "mainnet")
                .await
                .expect("reconcile");

        let rows = store.list_offer_state_details("m1", 20).expect("rows");
        let row = rows
            .into_iter()
            .find(|entry| entry.offer_id == "offer-50")
            .expect("offer row");
        let transitions = store
            .list_recent_audit_events(Some(&["offer_lifecycle_transition"]), Some("m1"), 20)
            .expect("audit");

        assert_eq!(row.state, "expired");
        assert!(row.last_seen_status.is_none());
        assert_eq!(transitions[0].payload["offer_id"], "offer-50");
        assert_eq!(
            transitions[0].payload["signal_source"],
            "dexie_get_offer_404"
        );
        assert!(result.metrics.immediate_requeue_requested);
        assert!(result
            .metrics
            .immediate_requeue_signals
            .iter()
            .any(|signal| signal.contains("expired")));
    }

    #[tokio::test]
    async fn reconcile_requests_immediate_requeue_on_dexie_status_confirmed() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        store
            .upsert_offer_state("offer-confirmed", "m1", "open", Some(0))
            .expect("seed");

        let mut server = mockito::Server::new_async().await;
        let _list = server
            .mock("GET", Matcher::Regex(r"/v1/offers\?.*".to_string()))
            .with_status(200)
            .with_body(
                r#"{"success":true,"offers":[{"id":"offer-confirmed","status":4,"offered":[{"asset_id":"asset1","amount":50000}],"requested":[{"asset_id":"xch","amount":1000}]}]}"#,
            )
            .create();
        let dexie = DexieClient::new(server.url());
        let result =
            run_reconcile_market_cycle(&store, &dexie, &sample_market("asset1", "xch"), "mainnet")
                .await
                .expect("reconcile");

        let rows = store.list_offer_state_details("m1", 20).expect("rows");
        let row = rows
            .into_iter()
            .find(|entry| entry.offer_id == "offer-confirmed")
            .expect("offer row");

        assert_eq!(row.state, "tx_block_confirmed");
        assert_eq!(row.last_seen_status, Some(4));
        assert!(result.metrics.immediate_requeue_requested);
        assert!(result
            .metrics
            .immediate_requeue_signals
            .iter()
            .any(|signal| signal.contains("tx_confirmed")));
    }

    #[tokio::test]
    async fn reconcile_resolves_xch_quote_before_dexie_fetch() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");

        let mut server = mockito::Server::new_async().await;
        let _list = server
            .mock(
                "GET",
                Matcher::Regex(r"/v1/offers\?offered=asset1&requested=xch".to_string()),
            )
            .with_status(200)
            .with_body(r#"{"success":true,"offers":[]}"#)
            .create();
        let dexie = DexieClient::new(server.url());
        run_reconcile_market_cycle(&store, &dexie, &sample_market("asset1", "xch"), "mainnet")
            .await
            .expect("reconcile");
    }

    #[tokio::test]
    async fn reconcile_dexie_fallback_status_does_not_mark_mempool() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        store
            .upsert_offer_state("offer-open", "m1", "open", Some(0))
            .expect("seed");

        let mut server = mockito::Server::new_async().await;
        let _list = server
            .mock("GET", Matcher::Regex(r"/v1/offers\?.*".to_string()))
            .with_status(200)
            .with_body(r#"{"success":true,"offers":[{"id":"offer-open","status":5}]}"#)
            .create();
        let dexie = DexieClient::new(server.url());
        run_reconcile_market_cycle(&store, &dexie, &sample_market("asset1", "xch"), "mainnet")
            .await
            .expect("reconcile");

        let rows = store.list_offer_state_details("m1", 20).expect("rows");
        let row = rows
            .into_iter()
            .find(|entry| entry.offer_id == "offer-open")
            .expect("offer row");

        assert_eq!(row.state, "open");
        assert_eq!(row.last_seen_status, Some(5));
    }
}
