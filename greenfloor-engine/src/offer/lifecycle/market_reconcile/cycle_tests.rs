use mockito::Matcher;
use tempfile::TempDir;

use super::*;
use crate::adapters::DexieClient;
use crate::config::MarketConfig;
use crate::storage::{OfferCancelWrite, OfferStateListRow, SqliteStore};
use crate::test_support::market_config::sample_market;

struct Harness {
    _dir: TempDir,
    store: SqliteStore,
    server: mockito::ServerGuard,
    market: MarketConfig,
}

impl Harness {
    async fn new(base_asset: &str, quote_asset: &str) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let server = mockito::Server::new_async().await;
        let mut market = sample_market("xch1test");
        market.base_asset = base_asset.to_string();
        market.base_symbol = "AS1".to_string();
        market.quote_asset = quote_asset.to_string();
        Self {
            _dir: dir,
            store,
            server,
            market,
        }
    }

    fn seed_offer(&self, offer_id: &str, venue: Option<&str>) {
        self.store
            .upsert_offer_state_with_metadata_at(
                offer_id,
                "m1",
                "open",
                Some(0),
                &chrono::Utc::now().to_rfc3339(),
                OfferCancelWrite {
                    listing: crate::storage::OfferListingWrite::venue(venue),
                    ..Default::default()
                },
            )
            .expect("seed");
    }

    fn mock_list(&mut self, body: &str) -> mockito::Mock {
        self.server
            .mock("GET", Matcher::Regex(r"/v1/offers\?.*".to_string()))
            .with_status(200)
            .with_body(body)
            .create()
    }

    fn mock_list_url(&mut self, url_regex: &str, body: &str) -> mockito::Mock {
        self.server
            .mock("GET", Matcher::Regex(url_regex.to_string()))
            .with_status(200)
            .with_body(body)
            .create()
    }

    fn mock_get_404(&mut self, offer_id: &str) -> mockito::Mock {
        self.server
            .mock("GET", format!("/v1/offers/{offer_id}").as_str())
            .with_status(404)
            .with_body(r#"{"success":false,"error":"Not Found"}"#)
            .create()
    }

    async fn run(&self) -> ReconcileMarketCycleResult {
        let dexie = DexieClient::new(self.server.url());
        run_reconcile_market_cycle(&self.store, &dexie, &self.market, "mainnet")
            .await
            .expect("reconcile")
    }

    fn offer_row(&self, offer_id: &str) -> OfferStateListRow {
        self.store
            .list_offer_states(Some("m1"), 20)
            .expect("rows")
            .into_iter()
            .find(|entry| entry.offer_id == offer_id)
            .expect("offer row")
    }
}

#[tokio::test]
async fn reconcile_expires_watched_offer_on_dexie_404() {
    let mut h = Harness::new("asset1", "xch").await;
    h.seed_offer("offer-50", Some("dexie"));
    let _list = h.mock_list(r#"{"success":true,"offers":[]}"#);
    let _single = h.mock_get_404("offer-50");

    let result = h.run().await;
    let row = h.offer_row("offer-50");
    let transitions = h
        .store
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
    let mut h = Harness::new("asset1", "xch").await;
    let offer_id = "ab".repeat(32);
    h.seed_offer(&offer_id, Some("coinset"));
    let _list = h.mock_list(r#"{"success":true,"offers":[]}"#);
    let _single = h.mock_get_404(&offer_id);

    let result = h.run().await;
    assert_eq!(h.offer_row(&offer_id).state, "open");
    assert!(!result.metrics.immediate_requeue_requested);
}

#[tokio::test]
async fn reconcile_null_venue_heals_watches_from_dexie_without_lifecycle() {
    let mut h = Harness::new("asset1", "xch").await;
    let offer_id = "ab".repeat(32);
    let coin = "cd".repeat(32);
    h.seed_offer(&offer_id, None);
    assert!(!h.store.offer_has_coin_watches(&offer_id).expect("none"));

    let body = format!(
        r#"{{"success":true,"offers":[{{"id":"{offer_id}","trade_id":"0x{offer_id}","status":0,"coins":[{{"coin_id":"0x{coin}"}}],"offered":[{{"asset_id":"asset1","amount":50000}}],"requested":[{{"asset_id":"xch","amount":1000}}]}}]}}"#
    );
    let _list = h.mock_list(&body);

    let result = h.run().await;
    let row = h.offer_row(&offer_id);
    // Heal-only: watches seeded, lifecycle stays open (not Dexie-authoritative).
    assert_eq!(row.state, "open");
    assert!(h.store.offer_has_coin_watches(&offer_id).expect("healed"));
    assert!(h
        .store
        .list_watched_coin_ids_for_market("m1")
        .expect("coins")
        .contains(&coin));
    assert!(result.dexie_size_by_offer_id.is_empty());
    assert!(result.dexie_status_by_lookup_key.is_empty());
    assert!(!result.metrics.immediate_requeue_requested);
}

#[tokio::test]
async fn reconcile_matches_dexie_trade_id_to_local_offer_id() {
    let mut h = Harness::new("asset1", "xch").await;
    let trade_id = "ab".repeat(32);
    let bech32_id = "7hj4tAYZEm9xTTniZiEVsPZ3mAnWvdposXizL3kDcjvo";
    h.seed_offer(&trade_id, Some("dexie"));
    let body = format!(
        r#"{{"success":true,"offers":[{{"id":"{bech32_id}","trade_id":"0x{trade_id}","status":4,"offered":[{{"asset_id":"asset1","amount":50000}}],"requested":[{{"asset_id":"xch","amount":1000}}]}}]}}"#
    );
    let _list = h.mock_list(&body);

    let result = h.run().await;
    let row = h.offer_row(&trade_id);
    assert_eq!(row.state, "tx_block_confirmed");
    assert_eq!(row.last_seen_status, Some(4));
    assert!(result.metrics.immediate_requeue_requested);
}

#[tokio::test]
async fn reconcile_requests_immediate_requeue_on_dexie_status_confirmed() {
    let mut h = Harness::new("asset1", "xch").await;
    h.seed_offer("offer-confirmed", Some("dexie"));
    let _list = h.mock_list(
        r#"{"success":true,"offers":[{"id":"offer-confirmed","status":4,"offered":[{"asset_id":"asset1","amount":50000}],"requested":[{"asset_id":"xch","amount":1000}]}]}"#,
    );

    let result = h.run().await;
    let row = h.offer_row("offer-confirmed");
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
    let mut h = Harness::new("asset1", "xch").await;
    h.seed_offer("offer-open", Some("dexie"));
    let _list = h.mock_list_url(
        r"/v1/offers\?offered=asset1&requested=xch",
        r#"{"success":true,"offers":[]}"#,
    );
    h.run().await;
}

#[tokio::test]
async fn reconcile_dexie_fallback_status_does_not_mark_mempool() {
    let mut h = Harness::new("asset1", "xch").await;
    h.seed_offer("offer-open", Some("dexie"));
    let _list = h.mock_list(r#"{"success":true,"offers":[{"id":"offer-open","status":5}]}"#);

    h.run().await;
    let row = h.offer_row("offer-open");
    assert_eq!(row.state, "open");
    assert_eq!(row.last_seen_status, Some(5));
}
