//! Unique maker pin session: distinct receive-address makers for Direct offers.

use std::collections::HashSet;

use crate::coinset::{is_xch_like_asset, list_wallet_unspent_coins_for_signer, WalletUnspentCoin};
use crate::config::{MarketConfig, SignerConfig};
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::offer::assets::ResolvedMarketOfferAssets;
use crate::offer::request::compute_signer_offer_leg_amounts;
use crate::offer::types::{effective_maker_reuse, PresplitMakerReuse};
use crate::storage::SqliteStore;

/// Borrowed inputs for one [`UniqueMakerPinSession::pin_after_bootstrap`] call.
#[derive(Debug, Clone, Copy)]
pub(crate) struct UniqueMakerPinRequest<'a> {
    pub market: &'a MarketConfig,
    pub assets: &'a ResolvedMarketOfferAssets,
    pub size_base_units: u64,
    pub side: &'a str,
    pub operator_network: &'a str,
    pub signer: &'a SignerConfig,
    /// When set (tests only), skip Coinset and use this pin result.
    #[cfg(test)]
    pub test_pin_result: Option<&'a Result<String, String>>,
}

/// In-batch protocol that pins a distinct exact-size receive-address maker coin per Direct offer.
///
/// Callers that need a live session acquire a `SQLite` connection and call [`Self::begin`].
/// When live pin is gated off, use [`Self::inactive`] (or `begin`, which returns inactive without
/// reading the store). Inactive sessions no-op `pin` / `commit`.
#[derive(Debug, Default)]
pub(crate) struct UniqueMakerPinSession {
    live: bool,
    excludes: HashSet<String>,
    pending: Vec<String>,
}

impl UniqueMakerPinSession {
    /// Whether this post should live-pin via Coinset (excludes dry-run / flag off / `PreferExisting`).
    #[must_use]
    pub(crate) fn needs_live(
        dry_run: bool,
        unique_maker_coins: bool,
        maker_reuse: Option<&PresplitMakerReuse>,
    ) -> bool {
        !dry_run && unique_maker_coins && effective_maker_reuse(maker_reuse).is_none()
    }

    /// Inactive session: pin/commit are no-ops (dry-run, flag off, or `PreferExisting`).
    #[must_use]
    pub(crate) fn inactive() -> Self {
        Self {
            live: false,
            excludes: HashSet::new(),
            pending: Vec::new(),
        }
    }

    /// Begin a batch session. When live pin is gated on, loads binding excludes from `store`.
    /// When gated off, returns [`Self::inactive`] without reading the store.
    ///
    /// # Errors
    ///
    /// Returns an error when live pin is gated on and `market_id` is empty or the binding query fails.
    pub(crate) fn begin(
        store: &SqliteStore,
        market_id: &str,
        dry_run: bool,
        unique_maker_coins: bool,
        maker_reuse: Option<&PresplitMakerReuse>,
    ) -> SignerResult<Self> {
        if !Self::needs_live(dry_run, unique_maker_coins, maker_reuse) {
            return Ok(Self::inactive());
        }
        let binding = load_binding_maker_coin_ids(store, market_id)?;
        let mut excludes = HashSet::with_capacity(binding.len());
        for coin_id in &binding {
            record_exclude(&mut excludes, coin_id);
        }
        Ok(Self {
            live: true,
            excludes,
            pending: Vec::new(),
        })
    }

    /// Whether this session will live-pin via Coinset (false ⇒ pin/commit no-op).
    #[cfg(test)]
    #[must_use]
    pub(crate) fn is_live(&self) -> bool {
        self.live
    }

    /// Whether `coin_id` is in the binding / committed-session exclude set.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn has_exclude(&self, coin_id: &str) -> bool {
        let id = normalize_hex_id(coin_id);
        !id.is_empty() && self.excludes.contains(&id)
    }

    /// Pin after bootstrap so denomination shaping cannot spend the chosen coin first.
    ///
    /// On success, pending ids are available via [`Self::pending_offer_coin_ids`]. Failures clear
    /// pending and return [`SignerError`]; callers map to soft-fail post outcomes.
    ///
    /// # Errors
    ///
    /// Returns an error when leg pricing fails, Coinset listing fails, or no free exact-size
    /// coin remains.
    pub(crate) async fn pin_after_bootstrap(
        &mut self,
        request: UniqueMakerPinRequest<'_>,
    ) -> SignerResult<()> {
        self.pending.clear();
        if !self.live {
            return Ok(());
        }
        #[cfg(test)]
        if let Some(result) = request.test_pin_result {
            return match result {
                Ok(coin_id) => {
                    let id = normalize_hex_id(coin_id);
                    if id.is_empty() {
                        return Err(SignerError::Other(
                            "unique pin test stub returned empty coin id".to_string(),
                        ));
                    }
                    self.pending = vec![id];
                    Ok(())
                }
                Err(message) => Err(SignerError::Other(message.clone())),
            };
        }
        let ids = resolve_unique_offer_coin_ids(
            request.market,
            request.assets,
            request.size_base_units,
            request.side,
            request.operator_network,
            request.signer,
            &self.excludes,
        )
        .await?;
        self.pending = ids;
        Ok(())
    }

    /// Pending `offer_coin_ids` from the last successful [`Self::pin_after_bootstrap`].
    #[must_use]
    pub(crate) fn pending_offer_coin_ids(&self) -> &[String] {
        &self.pending
    }

    /// Move pending pins into the batch exclude set after a successful venue publish.
    ///
    /// On failure (or inactive session), clears pending without touching excludes so a later
    /// `repeat` iteration can reuse the coin.
    pub(crate) fn commit_on_publish(&mut self, success: bool) {
        if !self.live || !success {
            self.pending.clear();
            return;
        }
        for coin_id in self.pending.drain(..) {
            record_exclude(&mut self.excludes, &coin_id);
        }
    }
}

fn load_binding_maker_coin_ids(store: &SqliteStore, market_id: &str) -> SignerResult<Vec<String>> {
    let market_id = market_id.trim();
    if market_id.is_empty() {
        return Err(SignerError::Other(
            "market_id required to load unique-maker bindings".to_string(),
        ));
    }
    store.list_binding_maker_coin_ids(market_id)
}

fn offered_leg_for_unique_pin(
    market: &MarketConfig,
    assets: &ResolvedMarketOfferAssets,
    size_base_units: u64,
    side: &str,
) -> SignerResult<(String, u64)> {
    let quote_price = market.quote_price_for_side(side)?;
    let size_i64 = i64::try_from(size_base_units).map_err(|_| SignerError::InvalidSizeBaseUnits)?;
    let leg = compute_signer_offer_leg_amounts(
        size_i64,
        quote_price,
        &assets.base_asset_id,
        &assets.quote_asset_id,
        side,
        &market.pricing,
    )?;
    Ok((leg.offer_asset_id, leg.offer_amount_mojos))
}

async fn resolve_unique_offer_coin_ids(
    market: &MarketConfig,
    assets: &ResolvedMarketOfferAssets,
    size_base_units: u64,
    side: &str,
    operator_network: &str,
    signer: &SignerConfig,
    excludes: &HashSet<String>,
) -> SignerResult<Vec<String>> {
    let (offered_asset_id, target_amount_mojos) =
        offered_leg_for_unique_pin(market, assets, size_base_units, side)?;
    let coin_id = pin_unique_exact_maker_coin_id(
        operator_network,
        signer,
        &market.receive_address,
        &offered_asset_id,
        target_amount_mojos,
        excludes,
    )
    .await?;
    Ok(vec![coin_id])
}

async fn pin_unique_exact_maker_coin_id(
    operator_network: &str,
    signer: &SignerConfig,
    receive_address: &str,
    offered_asset_id: &str,
    target_amount_mojos: u64,
    excludes: &HashSet<String>,
) -> SignerResult<String> {
    let coins = list_wallet_unspent_coins_for_signer(
        operator_network,
        signer,
        receive_address,
        offered_asset_id,
    )
    .await?;
    pick_from_unspent(&coins, excludes, offered_asset_id, target_amount_mojos)
}

fn record_exclude(excludes: &mut HashSet<String>, coin_id: &str) {
    let id = normalize_hex_id(coin_id);
    if !id.is_empty() {
        excludes.insert(id);
    }
}

fn pick_from_unspent(
    coins: &[WalletUnspentCoin],
    excludes: &HashSet<String>,
    offered_asset_id: &str,
    target_amount_mojos: u64,
) -> SignerResult<String> {
    let free: Vec<&WalletUnspentCoin> = coins
        .iter()
        .filter(|coin| {
            let id = normalize_hex_id(&coin.id);
            !id.is_empty() && !excludes.contains(&id)
        })
        .collect();
    if free.is_empty() {
        return Err(empty_unspent_err(offered_asset_id));
    }
    free.iter()
        .find(|coin| coin.amount == target_amount_mojos)
        .map(|coin| normalize_hex_id(&coin.id))
        .filter(|id| !id.is_empty())
        .ok_or_else(|| insufficient_err(offered_asset_id))
}

fn empty_unspent_err(offered_asset_id: &str) -> SignerError {
    if is_xch_like_asset(offered_asset_id) {
        SignerError::NoUnspentOfferXchCoins
    } else {
        SignerError::NoUnspentCatCoins
    }
}

fn insufficient_err(offered_asset_id: &str) -> SignerError {
    if is_xch_like_asset(offered_asset_id) {
        SignerError::InsufficientOfferXchCoins
    } else {
        SignerError::InsufficientCatCoins
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MarketPricing;
    use crate::offer::types::{OfferCancelFields, OfferExecutionMode};
    use crate::storage::{OfferCancelWrite, OfferListingWrite};
    use crate::test_support::market_config::sample_market;
    use crate::test_support::signer_config::test_signer_config;
    use tempfile::tempdir;

    fn coin(id_byte: u8, amount: u64) -> WalletUnspentCoin {
        let id = hex::encode([id_byte; 32]);
        WalletUnspentCoin {
            id: id.clone(),
            name: id,
            amount,
            state: "CONFIRMED".to_string(),
            puzzle_hash: hex::encode([0u8; 32]),
        }
    }

    #[test]
    fn pick_excludes_bound_coin_and_returns_exact_size() {
        let coins = vec![coin(0xaa, 10_000), coin(0xbb, 10_000), coin(0xcc, 40_000)];
        let excludes = HashSet::from([hex::encode([0xaa; 32])]);
        let picked = pick_from_unspent(&coins, &excludes, &"ab".repeat(32), 10_000).expect("pick");
        assert_eq!(picked, hex::encode([0xbb; 32]));
    }

    #[test]
    fn pick_fails_closed_when_only_oversize_coins_remain() {
        let coins = vec![coin(0xaa, 5_000), coin(0xbb, 20_000), coin(0xcc, 15_000)];
        let excludes = HashSet::new();
        let err =
            pick_from_unspent(&coins, &excludes, &"ab".repeat(32), 10_000).expect_err("exact");
        assert!(matches!(err, SignerError::InsufficientCatCoins));
    }

    #[test]
    fn pick_errors_when_only_excluded_exact_coins_remain() {
        let coins = vec![coin(0xaa, 10_000)];
        let excludes = HashSet::from([hex::encode([0xaa; 32])]);
        let err =
            pick_from_unspent(&coins, &excludes, &"ab".repeat(32), 10_000).expect_err("empty");
        assert!(matches!(err, SignerError::NoUnspentCatCoins));
    }

    #[test]
    fn pick_xch_errors_when_empty_or_only_oversize() {
        let excludes = HashSet::new();
        let empty_err = pick_from_unspent(&[], &excludes, "xch", 10_000).expect_err("empty");
        assert!(matches!(empty_err, SignerError::NoUnspentOfferXchCoins));
        let oversize = vec![coin(0xaa, 20_000)];
        let insuf =
            pick_from_unspent(&oversize, &excludes, "xch", 10_000).expect_err("insufficient");
        assert!(matches!(insuf, SignerError::InsufficientOfferXchCoins));
    }

    #[test]
    fn two_sequential_picks_get_distinct_exact_makers() {
        let coins = vec![coin(0xaa, 10_000), coin(0xbb, 10_000)];
        let asset = "ab".repeat(32);
        let first = pick_from_unspent(&coins, &HashSet::new(), &asset, 10_000).expect("first");
        let mut excludes = HashSet::from([first.clone()]);
        let second = pick_from_unspent(&coins, &excludes, &asset, 10_000).expect("second");
        assert_ne!(first, second);
        record_exclude(&mut excludes, &second);
        assert_eq!(excludes.len(), 2);
    }

    #[test]
    fn begin_inactive_when_gated_off() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let dry = UniqueMakerPinSession::begin(&store, "m1", true, true, None).expect("dry");
        assert!(!dry.is_live());
        let disabled = UniqueMakerPinSession::begin(&store, "m1", false, false, None).expect("off");
        assert!(!disabled.is_live());
        let reuse = PresplitMakerReuse {
            coin_id: "aa".repeat(32),
            offer_nonce: "bb".repeat(32),
        };
        let existing =
            UniqueMakerPinSession::begin(&store, "m1", false, true, Some(&reuse)).expect("reuse");
        assert!(!existing.is_live());
        assert!(!UniqueMakerPinSession::inactive().is_live());
        assert!(!UniqueMakerPinSession::needs_live(true, true, None));
        assert!(UniqueMakerPinSession::needs_live(false, true, None));
    }

    #[test]
    fn begin_loads_bindings_when_live() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let coin = hex::encode([0xaa; 32]);
        let fields =
            OfferCancelFields::from_presplit_build(coin.clone(), "bb".repeat(32), "cc".repeat(32));
        store
            .upsert_offer_state_with_metadata_at(
                "o1",
                "m1",
                "open",
                None,
                "2026-01-01T00:00:00Z",
                OfferCancelWrite {
                    fields: Some(&fields),
                    execution_mode: Some(OfferExecutionMode::Direct),
                    listing: OfferListingWrite {
                        publish_venue: Some("dexie"),
                        listing_expires_at: None,
                        size_base_units: Some(10),
                        offer_nonce: None,
                        offer_side: Some("sell"),
                    },
                    ..OfferCancelWrite::default()
                },
            )
            .expect("upsert");
        let session = UniqueMakerPinSession::begin(&store, "m1", false, true, None).expect("begin");
        assert!(session.is_live());
        assert!(session.has_exclude(&coin));
    }

    #[test]
    fn begin_fails_closed_on_empty_market() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let err = UniqueMakerPinSession::begin(&store, "  ", false, true, None).expect_err("empty");
        assert!(err.to_string().contains("market_id required"));
    }

    #[tokio::test]
    async fn pin_then_commit_on_publish_only_after_success() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let mut session =
            UniqueMakerPinSession::begin(&store, "m1", false, true, None).expect("begin");
        let market =
            sample_market("xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h");
        let assets = ResolvedMarketOfferAssets {
            base_asset_id: "ab".repeat(32),
            quote_asset_id: "xch".to_string(),
            quote_asset_for_offer: "xch".to_string(),
        };
        let signer = test_signer_config("http://127.0.0.1:9");
        let coin = "bb".repeat(32);
        let stub_ok = Ok(coin.clone());
        session
            .pin_after_bootstrap(UniqueMakerPinRequest {
                market: &market,
                assets: &assets,
                size_base_units: 10,
                side: "sell",
                operator_network: "mainnet",
                signer: &signer,
                test_pin_result: Some(&stub_ok),
            })
            .await
            .expect("pin");
        session.commit_on_publish(false);
        assert!(!session.has_exclude(&coin));
        assert!(session.pending_offer_coin_ids().is_empty());

        session
            .pin_after_bootstrap(UniqueMakerPinRequest {
                market: &market,
                assets: &assets,
                size_base_units: 10,
                side: "sell",
                operator_network: "mainnet",
                signer: &signer,
                test_pin_result: Some(&stub_ok),
            })
            .await
            .expect("pin again");
        session.commit_on_publish(true);
        assert!(session.has_exclude(&coin));
        assert!(session.pending_offer_coin_ids().is_empty());
    }

    #[tokio::test]
    async fn pin_test_stub_sets_pending_without_commit() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let mut session =
            UniqueMakerPinSession::begin(&store, "m1", false, true, None).expect("begin");
        let market =
            sample_market("xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h");
        let assets = ResolvedMarketOfferAssets {
            base_asset_id: "ab".repeat(32),
            quote_asset_id: "xch".to_string(),
            quote_asset_for_offer: "xch".to_string(),
        };
        let signer = test_signer_config("http://127.0.0.1:9");
        let stub = Ok("aa".repeat(32));
        session
            .pin_after_bootstrap(UniqueMakerPinRequest {
                market: &market,
                assets: &assets,
                size_base_units: 10,
                side: "sell",
                operator_network: "mainnet",
                signer: &signer,
                test_pin_result: Some(&stub),
            })
            .await
            .expect("stub pin");
        assert_eq!(session.pending_offer_coin_ids(), &["aa".repeat(32)]);
        assert!(!session.has_exclude("aa".repeat(32).as_str()));
    }

    #[test]
    fn offered_leg_for_unique_pin_matches_create_path() {
        let cat = "ab".repeat(32);
        let mut market =
            sample_market("xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h");
        market.base_asset = cat.clone();
        market.pricing = MarketPricing {
            fixed_quote_per_base: Some(1.0),
            base_unit_mojo_multiplier: Some(1_000),
            quote_unit_mojo_multiplier: Some(1_000_000_000_000),
            ..MarketPricing::default()
        };
        let assets = ResolvedMarketOfferAssets {
            base_asset_id: cat.clone(),
            quote_asset_id: "xch".to_string(),
            quote_asset_for_offer: "xch".to_string(),
        };
        let (asset, amount) =
            offered_leg_for_unique_pin(&market, &assets, 10, "sell").expect("leg");
        assert_eq!(asset, cat);
        assert_eq!(amount, 10_000);
    }

    #[test]
    fn offered_leg_for_unique_pin_buy_uses_bid_price() {
        let base = "ab".repeat(32);
        let quote = "cd".repeat(32);
        let mut market =
            sample_market("xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h");
        market.mode = "two_sided".to_string();
        market.base_asset = base.clone();
        market.quote_asset = quote.clone();
        market.pricing = MarketPricing {
            fixed_quote_per_base: Some(1.0),
            strategy_target_spread_bps: Some(20),
            base_unit_mojo_multiplier: Some(1_000),
            quote_unit_mojo_multiplier: Some(1_000),
            ..MarketPricing::default()
        };
        let assets = ResolvedMarketOfferAssets {
            base_asset_id: base,
            quote_asset_id: quote.clone(),
            quote_asset_for_offer: quote.clone(),
        };
        let (asset, amount) = offered_leg_for_unique_pin(&market, &assets, 10, "buy").expect("leg");
        assert_eq!(asset, quote);
        assert_eq!(amount, 9_990);
    }

    #[tokio::test]
    async fn pin_unique_exact_maker_coin_id_lists_and_picks_exact_xch() {
        const RECEIVE: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";
        let body = r#"{
            "success": true,
            "coin_records": [{
                "coin": {
                    "parent_coin_info": "c325057d788bee13367cb8e2d71ff3e209b5e94b31b296322ba1a143053fef5b",
                    "puzzle_hash": "11cd056d9ec93f4612919b445e1ad9afeb7ef7739708c2d16cec4fd2d3cd5e63",
                    "amount": 5000
                },
                "coinbase": false,
                "confirmed_block_index": 1,
                "spent": false,
                "spent_block_index": 0,
                "timestamp": 1
            }]
        }"#;
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(200)
            .with_body(body)
            .expect_at_least(2)
            .create_async()
            .await;
        let signer = test_signer_config(&server.url());
        let pinned = pin_unique_exact_maker_coin_id(
            "mainnet",
            &signer,
            RECEIVE,
            "xch",
            5000,
            &HashSet::new(),
        )
        .await
        .expect("pin exact xch");
        assert_eq!(pinned.len(), 64);
        let excluded = HashSet::from([pinned]);
        let err =
            pin_unique_exact_maker_coin_id("mainnet", &signer, RECEIVE, "xch", 5000, &excluded)
                .await
                .expect_err("excluded");
        assert!(matches!(err, SignerError::NoUnspentOfferXchCoins));
    }

    #[tokio::test]
    async fn pin_unique_exact_maker_coin_id_empty_cat_list_fails_closed() {
        const RECEIVE: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";
        let asset = "ab".repeat(32);
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(200)
            .with_body(r#"{"success":true,"coin_records":[]}"#)
            .create_async()
            .await;
        let signer = test_signer_config(&server.url());
        let err = pin_unique_exact_maker_coin_id(
            "mainnet",
            &signer,
            RECEIVE,
            &asset,
            10_000,
            &HashSet::new(),
        )
        .await
        .expect_err("empty");
        assert!(matches!(err, SignerError::NoUnspentCatCoins));
    }
}
