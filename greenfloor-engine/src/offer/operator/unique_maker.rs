//! Pin distinct receive-address maker coins for Direct offers (`unique_maker_coins`).

use std::collections::HashSet;

use crate::coinset::{is_xch_like_asset, list_wallet_unspent_coins_for_signer, WalletUnspentCoin};
use crate::config::{MarketConfig, SignerConfig};
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::offer::assets::ResolvedMarketOfferAssets;
use crate::offer::build_context::resolve_quote_price_for_pricing;
use crate::offer::request::compute_signer_offer_leg_amounts;
use crate::offer::types::{effective_maker_reuse, PresplitMakerReuse};
use crate::storage::SqliteStore;

/// Whether this post should live-pin a unique exact-size maker via Coinset.
#[must_use]
pub(crate) fn needs_live_unique_pin(
    dry_run: bool,
    unique_maker_coins: bool,
    maker_reuse: Option<&PresplitMakerReuse>,
) -> bool {
    !dry_run && unique_maker_coins && effective_maker_reuse(maker_reuse).is_none()
}

/// Seed the in-batch exclude set from preloaded binding coin ids (callers load `SQLite`).
#[must_use]
pub(crate) fn session_excludes_for_unique_pin(
    dry_run: bool,
    unique_maker_coins: bool,
    maker_reuse: Option<&PresplitMakerReuse>,
    binding_excludes: &[String],
) -> HashSet<String> {
    if !needs_live_unique_pin(dry_run, unique_maker_coins, maker_reuse) {
        return HashSet::new();
    }
    let mut out = HashSet::with_capacity(binding_excludes.len());
    for coin_id in binding_excludes {
        record_session_pin(&mut out, coin_id);
    }
    out
}

/// Load binding maker coin ids for a market. Empty `market_id` fails closed.
///
/// # Errors
///
/// Returns an error when `market_id` is empty or the query fails.
pub(crate) fn load_binding_maker_coin_ids(
    store: &SqliteStore,
    market_id: &str,
) -> SignerResult<Vec<String>> {
    let market_id = market_id.trim();
    if market_id.is_empty() {
        return Err(SignerError::Other(
            "market_id required to load unique-maker bindings".to_string(),
        ));
    }
    store.list_binding_maker_coin_ids(market_id)
}

/// Offered-leg asset id and mojo amount for a unique Direct pin (same multipliers as create).
///
/// # Errors
///
/// Returns an error when size/pricing cannot form a valid offered leg.
fn offered_leg_for_unique_pin(
    market: &MarketConfig,
    assets: &ResolvedMarketOfferAssets,
    size_base_units: u64,
    side: &str,
) -> SignerResult<(String, u64)> {
    let quote_price = resolve_quote_price_for_pricing(&market.pricing)?;
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

/// Resolve `offer_coin_ids` for a live unique Direct pin (leg target + Coinset pick).
///
/// Caller must only invoke when [`needs_live_unique_pin`] is true.
///
/// # Errors
///
/// Returns an error when leg pricing fails, Coinset listing fails, or no free exact-size
/// coin remains.
pub(crate) async fn resolve_unique_offer_coin_ids(
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

/// Pin one free exact-size receive coin, excluding binding + in-batch session coins.
///
/// # Errors
///
/// Returns an error when Coinset listing fails, or no free exact-size coin remains.
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

/// Record a pinned maker in the in-batch session exclude set.
pub(crate) fn record_session_pin(session_excludes: &mut HashSet<String>, coin_id: &str) {
    let id = normalize_hex_id(coin_id);
    if !id.is_empty() {
        session_excludes.insert(id);
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
        let mut session = HashSet::from([first.clone()]);
        let second = pick_from_unspent(&coins, &session, &asset, 10_000).expect("second");
        assert_ne!(first, second);
        record_session_pin(&mut session, &second);
        assert_eq!(session.len(), 2);
    }

    #[test]
    fn needs_live_unique_pin_gates() {
        assert!(!needs_live_unique_pin(true, true, None));
        assert!(!needs_live_unique_pin(false, false, None));
        let reuse = PresplitMakerReuse {
            coin_id: "aa".repeat(32),
            offer_nonce: "bb".repeat(32),
        };
        assert!(!needs_live_unique_pin(false, true, Some(&reuse)));
        assert!(needs_live_unique_pin(false, true, None));
    }

    #[test]
    fn session_excludes_for_unique_pin_honors_gate_and_normalizes() {
        let aa = hex::encode([0xaa; 32]);
        let binding = vec![format!("0x{aa}"), String::new()];
        let live = session_excludes_for_unique_pin(false, true, None, &binding);
        assert_eq!(live, HashSet::from([aa.clone()]));
        let dry = session_excludes_for_unique_pin(true, true, None, &binding);
        assert!(dry.is_empty());
        let disabled = session_excludes_for_unique_pin(false, false, None, &binding);
        assert!(disabled.is_empty());
    }

    #[test]
    fn load_binding_maker_coin_ids_fails_closed_on_empty_market() {
        let dir = tempdir().expect("tempdir");
        let store = SqliteStore::open(&dir.path().join("state.db")).expect("open");
        let err = load_binding_maker_coin_ids(&store, "  ").expect_err("empty");
        assert!(err.to_string().contains("market_id required"));
    }

    #[test]
    fn load_binding_maker_coin_ids_returns_open_makers() {
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
        let ids = load_binding_maker_coin_ids(&store, "m1").expect("load");
        assert_eq!(ids, vec![coin]);
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

    #[tokio::test]
    async fn pin_unique_exact_maker_coin_id_lists_and_picks_exact_xch() {
        const RECEIVE: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";
        // Same fixture shape as coinset_spendable tests (amount 5000).
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
