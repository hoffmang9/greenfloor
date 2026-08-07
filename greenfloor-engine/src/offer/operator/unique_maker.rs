//! Pin distinct receive-address maker coins for Direct offers (`unique_maker_coins`).

use std::collections::HashSet;

use crate::coinset::{is_xch_like_asset, list_wallet_unspent_coins_for_signer, WalletUnspentCoin};
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::offer::types::{effective_maker_reuse, PresplitMakerReuse};

/// Inputs for resolving `offer_coin_ids` under `unique_maker_coins`.
pub struct UniqueMakerResolveRequest<'a> {
    pub unique_maker_coins: bool,
    pub maker_reuse: Option<&'a PresplitMakerReuse>,
    pub existing: Vec<String>,
    pub excludes: &'a [String],
    pub operator_network: &'a str,
    pub signer: &'a SignerConfig,
    pub receive_address: &'a str,
    pub offered_asset_id: &'a str,
    pub target_amount_mojos: u64,
}

/// When unique mode applies, pick one free exact-size maker and return it as
/// `offer_coin_ids`; otherwise return `existing` unchanged.
///
/// Callers load binding excludes synchronously (via
/// `SqliteStore::list_binding_maker_coin_ids`) before awaiting so futures stay `Send`.
///
/// # Errors
///
/// Returns an error when Coinset listing fails or no free exact-size coin remains.
pub async fn resolve_unique_maker_offer_coin_ids(
    request: UniqueMakerResolveRequest<'_>,
) -> SignerResult<Vec<String>> {
    if !request.unique_maker_coins
        || effective_maker_reuse(request.maker_reuse).is_some()
        || !request.existing.is_empty()
    {
        return Ok(request.existing);
    }
    let coin_id = pick_unique_direct_maker_coin_id(
        request.excludes,
        request.operator_network,
        request.signer,
        request.receive_address,
        request.offered_asset_id,
        request.target_amount_mojos,
    )
    .await?;
    Ok(vec![coin_id])
}

/// Pick a free exact-size receive-address coin given preloaded binding excludes.
///
/// Only pins `amount == target_amount_mojos` so unique mode never forces an oversize
/// coin into a split/CONDITIONS path.
///
/// # Errors
///
/// Returns an error when Coinset listing fails, no free coins remain, or no free
/// exact-size coin is available.
pub async fn pick_unique_direct_maker_coin_id(
    excludes: &[String],
    operator_network: &str,
    signer: &SignerConfig,
    receive_address: &str,
    offered_asset_id: &str,
    target_amount_mojos: u64,
) -> SignerResult<String> {
    let exclude_set: HashSet<String> = excludes.iter().cloned().collect();
    let coins = list_wallet_unspent_coins_for_signer(
        operator_network,
        signer,
        receive_address,
        offered_asset_id,
    )
    .await?;
    pick_from_unspent(&coins, &exclude_set, offered_asset_id, target_amount_mojos)
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
    fn two_sequential_picks_get_distinct_exact_makers() {
        let coins = vec![coin(0xaa, 10_000), coin(0xbb, 10_000)];
        let asset = "ab".repeat(32);
        let first = pick_from_unspent(&coins, &HashSet::new(), &asset, 10_000).expect("first");
        let second = pick_from_unspent(&coins, &HashSet::from([first.clone()]), &asset, 10_000)
            .expect("second");
        assert_ne!(first, second);
        let expected = [hex::encode([0xaa; 32]), hex::encode([0xbb; 32])];
        assert!(expected.contains(&first) && expected.contains(&second));
    }

    #[tokio::test]
    async fn resolve_skips_when_unique_disabled_or_ids_already_set() {
        let signer =
            crate::test_support::signer_config::test_signer_config("https://api.coinset.org");
        let existing = vec!["aa".repeat(32)];
        let asset = "ab".repeat(32);
        let out = resolve_unique_maker_offer_coin_ids(UniqueMakerResolveRequest {
            unique_maker_coins: false,
            maker_reuse: None,
            existing: existing.clone(),
            excludes: &[],
            operator_network: "mainnet",
            signer: &signer,
            receive_address: "xch1test",
            offered_asset_id: &asset,
            target_amount_mojos: 10_000,
        })
        .await
        .expect("resolve");
        assert_eq!(out, existing);

        let out = resolve_unique_maker_offer_coin_ids(UniqueMakerResolveRequest {
            unique_maker_coins: true,
            maker_reuse: None,
            existing: existing.clone(),
            excludes: &[],
            operator_network: "mainnet",
            signer: &signer,
            receive_address: "xch1test",
            offered_asset_id: &asset,
            target_amount_mojos: 10_000,
        })
        .await
        .expect("resolve");
        assert_eq!(out, existing);
    }
}
