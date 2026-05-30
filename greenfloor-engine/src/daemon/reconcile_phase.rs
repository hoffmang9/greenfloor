use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use crate::adapters::DexieClient;
use crate::config::{resolve_quote_asset_for_offer, resolve_trade_asset_for_network, MarketConfig};
use crate::cycle::{
    resolve_watched_offer_transition_from_signals, CycleOfferTransition,
};
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::coinset_tx::{
    build_dexie_size_by_offer_id, dexie_offer_status, extract_coinset_tx_ids_from_offer_payload,
};
use super::reconcile_offer::transition_from_list_offer_payload;
use super::reconcile_augment::augment_dexie_offers_for_watchlist;
use super::reconcile_persist::{persist_offer_lifecycle_transition, ReconcilePersistOptions};
use super::watchlist::cache::CoinWatchlistCache;
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

pub(crate) fn apply_reconcile_transition(
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
        ReconcilePersistOptions {
            action: "reconcile_coins_and_offers",
            venue: Some("dexie"),
            dexie_error,
        },
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

pub(crate) fn transition_from_dexie_offer_payload(
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
    coin_watchlist: &CoinWatchlistCache,
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

    let our_offer_ids: HashSet<String> = watchlist_offer_ids(store, market_id)?
        .into_iter()
        .collect();
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

    for raw in &augmented_offers {
        let Some(offer_id) = raw
            .as_object()
            .and_then(|obj| obj.get("id"))
            .and_then(|value| value.as_str())
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
            .map(String::as_str)
            .unwrap_or("open");
        let (transition, status) =
            transition_from_list_offer_payload(store, current_state, raw)?;
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

    update_market_coin_watchlist_from_offers(
        store,
        coin_watchlist,
        market_id,
        &augmented_offers,
    )?;

    Ok(ReconcilePhaseResult {
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
    use crate::daemon::watchlist::cache::CoinWatchlistCache;
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
            ladders: HashMap::new(),
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
        let watchlist = CoinWatchlistCache::new();

        let result = run_market_reconcile_phase(
            &store,
            &watchlist,
            &dexie,
            &sample_market("asset1", "xch"),
            "mainnet",
        )
        .await
        .expect("reconcile");

        let rows = store
            .list_offer_state_details("m1", 20)
            .expect("rows");
        let row = rows
            .into_iter()
            .find(|entry| entry.offer_id == "offer-50")
            .expect("offer row");
        let transitions = store
            .list_recent_audit_events(
                Some(&["offer_lifecycle_transition"]),
                Some("m1"),
                20,
            )
            .expect("audit");

        assert_eq!(row.state, "expired");
        assert!(row.last_seen_status.is_none());
        assert_eq!(transitions[0].payload["offer_id"], "offer-50");
        assert_eq!(transitions[0].payload["signal_source"], "dexie_get_offer_404");
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
        let watchlist = CoinWatchlistCache::new();

        let result = run_market_reconcile_phase(
            &store,
            &watchlist,
            &dexie,
            &sample_market("asset1", "xch"),
            "mainnet",
        )
        .await
        .expect("reconcile");

        let rows = store
            .list_offer_state_details("m1", 20)
            .expect("rows");
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
        let watchlist = CoinWatchlistCache::new();

        run_market_reconcile_phase(
            &store,
            &watchlist,
            &dexie,
            &sample_market("asset1", "xch"),
            "mainnet",
        )
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
        let watchlist = CoinWatchlistCache::new();

        run_market_reconcile_phase(
            &store,
            &watchlist,
            &dexie,
            &sample_market("asset1", "xch"),
            "mainnet",
        )
        .await
        .expect("reconcile");

        let rows = store
            .list_offer_state_details("m1", 20)
            .expect("rows");
        let row = rows
            .into_iter()
            .find(|entry| entry.offer_id == "offer-open")
            .expect("offer row");

        assert_eq!(row.state, "open");
        assert_eq!(row.last_seen_status, Some(5));
    }
}
