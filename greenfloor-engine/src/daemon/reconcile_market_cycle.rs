//! Per-market daemon-cycle reconcile: Dexie list fetch and lifecycle transitions.

use std::collections::HashMap;

use chrono::Utc;
use serde_json::json;
use tracing::Level;

use crate::adapters::DexieClient;
use crate::config::{resolve_quote_asset_for_offer, resolve_trade_asset_for_network, MarketConfig};
use crate::error::SignerResult;
use crate::operator_log::{LogContext, DEXIE_OFFERS_ERROR};
use crate::storage::SqliteStore;

use super::dexie_size::{build_dexie_size_by_offer_id, dexie_status_index};
use super::reconcile_augment::augment_dexie_offers_for_watchlist;
use super::reconcile_transition::note_reconcile_transition_side_effects;
use super::watch_plan::{fetch_and_ensure_watches, prepare_market_reconcile_local};
use crate::offer::lifecycle::{
    apply_cancel_submitted_rows, apply_watched_offer_signals, cancel_submitted_context_for_offer,
    coinset_signals_from_dexie_offer_payload, preload_cancel_submitted_contexts,
    ReconcilePersistOptions,
};

pub use super::reconcile_transition::ReconcileMarketCycleMetrics;

#[derive(Debug, Clone)]
pub struct ReconcileMarketCycleResult {
    pub dexie_size_by_offer_id: HashMap<String, i64>,
    /// Dexie status keyed by every list lookup key (`trade_id` ∪ bech32 `id`).
    /// Built once in reconcile; cancel consumes this map (no re-walk of JSON).
    pub dexie_status_by_lookup_key: HashMap<String, i64>,
    pub dexie_fetch_error: Option<String>,
    pub metrics: ReconcileMarketCycleMetrics,
}

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
        return Ok(ReconcileMarketCycleResult {
            dexie_size_by_offer_id: HashMap::default(),
            dexie_status_by_lookup_key: HashMap::default(),
            dexie_fetch_error: None,
            metrics,
        });
    }
    let plan = local.dexie;
    let mut state_by_offer_id = local.state_by_offer_id;

    let dexie_offered_asset = resolve_trade_asset_for_network(&market.base_asset, network);
    let dexie_requested_asset = resolve_quote_asset_for_offer(&market.quote_asset, network);
    let list_offers = match dexie
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
                dexie_size_by_offer_id: HashMap::default(),
                dexie_status_by_lookup_key: HashMap::default(),
                dexie_fetch_error: Some(err.to_string()),
                metrics,
            });
        }
    };

    // Heal-only: Dexie payloads → watches. No lifecycle.
    fetch_and_ensure_watches(
        dexie,
        store,
        market_id,
        &plan.heal_only,
        &list_offers,
        &mut metrics,
    )
    .await?;

    if plan.authoritative.is_empty() {
        return Ok(ReconcileMarketCycleResult {
            dexie_size_by_offer_id: HashMap::default(),
            dexie_status_by_lookup_key: HashMap::default(),
            dexie_fetch_error: None,
            metrics,
        });
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

fn apply_dexie_lifecycle_transitions(
    store: &SqliteStore,
    market_id: &str,
    by_local_id: &HashMap<String, serde_json::Value>,
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

    for (local_offer_id, raw) in by_local_id {
        let current_state = state_by_offer_id
            .get(local_offer_id)
            .map_or("open", String::as_str);
        let (status, signals) = coinset_signals_from_dexie_offer_payload(store, raw)?;
        let cancel_submitted = cancel_submitted_context_for_offer(
            store,
            local_offer_id,
            current_state,
            Some(&cancel_submitted_by_offer),
        )?;
        let transition = apply_watched_offer_signals(
            store,
            market_id,
            local_offer_id,
            current_state,
            status,
            signals,
            cancel_submitted.as_ref(),
            &options,
            status,
            Utc::now(),
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
            .upsert_offer_state_with_metadata_at(
                "offer-50",
                "m1",
                "open",
                Some(0),
                &chrono::Utc::now().to_rfc3339(),
                crate::storage::OfferCancelWrite {
                    publish_venue: Some("dexie"),
                    ..Default::default()
                },
            )
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
    async fn reconcile_does_not_expire_coinset_offer_on_dexie_404() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        let offer_id = "ab".repeat(32);
        store
            .upsert_offer_state_with_metadata_at(
                &offer_id,
                "m1",
                "open",
                Some(0),
                &chrono::Utc::now().to_rfc3339(),
                crate::storage::OfferCancelWrite {
                    publish_venue: Some("coinset"),
                    ..Default::default()
                },
            )
            .expect("seed");

        let mut server = mockito::Server::new_async().await;
        let _list = server
            .mock("GET", Matcher::Regex(r"/v1/offers\?.*".to_string()))
            .with_status(200)
            .with_body(r#"{"success":true,"offers":[]}"#)
            .create();
        let _single = server
            .mock("GET", format!("/v1/offers/{offer_id}").as_str())
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
            .find(|entry| entry.offer_id == offer_id)
            .expect("offer row");
        assert_eq!(row.state, "open");
        assert!(!result.metrics.immediate_requeue_requested);
    }

    #[tokio::test]
    async fn reconcile_null_venue_heals_watches_from_dexie_without_lifecycle() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        let offer_id = "ab".repeat(32);
        let coin = "cd".repeat(32);
        // Legacy NULL venue, no cancel metadata, no watches.
        store
            .upsert_offer_state_with_metadata_at(
                &offer_id,
                "m1",
                "open",
                Some(0),
                &chrono::Utc::now().to_rfc3339(),
                crate::storage::OfferCancelWrite {
                    publish_venue: None,
                    ..Default::default()
                },
            )
            .expect("seed");
        assert!(!store.offer_has_coin_watches(&offer_id).expect("none"));

        let mut server = mockito::Server::new_async().await;
        let body = format!(
            r#"{{"success":true,"offers":[{{"id":"{offer_id}","trade_id":"0x{offer_id}","status":0,"coins":[{{"coin_id":"0x{coin}"}}],"offered":[{{"asset_id":"asset1","amount":50000}}],"requested":[{{"asset_id":"xch","amount":1000}}]}}]}}"#
        );
        let _list = server
            .mock("GET", Matcher::Regex(r"/v1/offers\?.*".to_string()))
            .with_status(200)
            .with_body(body)
            .create();
        let dexie = DexieClient::new(server.url());
        let result =
            run_reconcile_market_cycle(&store, &dexie, &sample_market("asset1", "xch"), "mainnet")
                .await
                .expect("reconcile");

        let rows = store.list_offer_state_details("m1", 20).expect("rows");
        let row = rows
            .into_iter()
            .find(|entry| entry.offer_id == offer_id)
            .expect("offer row");
        // Heal-only: watches seeded, lifecycle stays open (not Dexie-authoritative).
        assert_eq!(row.state, "open");
        assert!(store.offer_has_coin_watches(&offer_id).expect("healed"));
        assert!(store
            .list_watched_coin_ids_for_market("m1")
            .expect("coins")
            .contains(&coin));
        assert!(result.dexie_size_by_offer_id.is_empty());
        assert!(result.dexie_status_by_lookup_key.is_empty());
        assert!(!result.metrics.immediate_requeue_requested);
    }

    #[tokio::test]
    async fn reconcile_matches_dexie_trade_id_to_local_offer_id() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        let trade_id = "ab".repeat(32);
        let bech32_id = "7hj4tAYZEm9xTTniZiEVsPZ3mAnWvdposXizL3kDcjvo";
        store
            .upsert_offer_state_with_metadata_at(
                &trade_id,
                "m1",
                "open",
                Some(0),
                &chrono::Utc::now().to_rfc3339(),
                crate::storage::OfferCancelWrite {
                    publish_venue: Some("dexie"),
                    ..Default::default()
                },
            )
            .expect("seed");

        let mut server = mockito::Server::new_async().await;
        let body = format!(
            r#"{{"success":true,"offers":[{{"id":"{bech32_id}","trade_id":"0x{trade_id}","status":4,"offered":[{{"asset_id":"asset1","amount":50000}}],"requested":[{{"asset_id":"xch","amount":1000}}]}}]}}"#
        );
        let _list = server
            .mock("GET", Matcher::Regex(r"/v1/offers\?.*".to_string()))
            .with_status(200)
            .with_body(body)
            .create();
        let dexie = DexieClient::new(server.url());
        let result =
            run_reconcile_market_cycle(&store, &dexie, &sample_market("asset1", "xch"), "mainnet")
                .await
                .expect("reconcile");

        let rows = store.list_offer_state_details("m1", 20).expect("rows");
        let row = rows
            .into_iter()
            .find(|entry| entry.offer_id == trade_id)
            .expect("offer row");
        assert_eq!(row.state, "tx_block_confirmed");
        assert_eq!(row.last_seen_status, Some(4));
        assert!(result.metrics.immediate_requeue_requested);
    }

    #[tokio::test]
    async fn reconcile_requests_immediate_requeue_on_dexie_status_confirmed() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("state.db");
        let store = SqliteStore::open(&db_path).expect("open");
        store
            .upsert_offer_state_with_metadata_at(
                "offer-confirmed",
                "m1",
                "open",
                Some(0),
                &chrono::Utc::now().to_rfc3339(),
                crate::storage::OfferCancelWrite {
                    publish_venue: Some("dexie"),
                    ..Default::default()
                },
            )
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
        store
            .upsert_offer_state_with_metadata_at(
                "offer-open",
                "m1",
                "open",
                Some(0),
                &chrono::Utc::now().to_rfc3339(),
                crate::storage::OfferCancelWrite {
                    publish_venue: Some("dexie"),
                    ..Default::default()
                },
            )
            .expect("seed");

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
            .upsert_offer_state_with_metadata_at(
                "offer-open",
                "m1",
                "open",
                Some(0),
                &chrono::Utc::now().to_rfc3339(),
                crate::storage::OfferCancelWrite {
                    publish_venue: Some("dexie"),
                    ..Default::default()
                },
            )
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
