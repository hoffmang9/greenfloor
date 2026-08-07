//! Pin distinct receive-address maker coins for Direct offers (`unique_maker_coins`).

use std::collections::HashSet;

use crate::coinset::{is_xch_like_asset, list_wallet_unspent_coins_for_signer, WalletUnspentCoin};
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::offer::types::{effective_maker_reuse, PresplitMakerReuse};

/// Whether this post should live-pin a unique exact-size maker via Coinset.
#[must_use]
pub(crate) fn needs_live_unique_pin(
    dry_run: bool,
    unique_maker_coins: bool,
    maker_reuse: Option<&PresplitMakerReuse>,
) -> bool {
    !dry_run && unique_maker_coins && effective_maker_reuse(maker_reuse).is_none()
}

/// Live pin inputs (caller already checked [`needs_live_unique_pin`] and owns excludes).
pub(crate) struct UniqueMakerLivePin<'a> {
    pub excludes: &'a HashSet<String>,
    pub operator_network: &'a str,
    pub signer: &'a SignerConfig,
    pub receive_address: &'a str,
    pub offered_asset_id: &'a str,
    pub target_amount_mojos: u64,
}

/// Pin one free exact-size receive coin, excluding binding + in-batch session coins.
///
/// # Errors
///
/// Returns an error when Coinset listing fails, or no free exact-size coin remains.
pub(crate) async fn pin_unique_exact_maker_coin_id(
    pin: UniqueMakerLivePin<'_>,
) -> SignerResult<String> {
    let coins = list_wallet_unspent_coins_for_signer(
        pin.operator_network,
        pin.signer,
        pin.receive_address,
        pin.offered_asset_id,
    )
    .await?;
    pick_from_unspent(
        &coins,
        pin.excludes,
        pin.offered_asset_id,
        pin.target_amount_mojos,
    )
}

/// Record a pinned maker in the in-batch session exclude set.
pub(crate) fn record_session_pin(session_excludes: &mut HashSet<String>, coin_id: &str) {
    let id = normalize_hex_id(coin_id);
    if !id.is_empty() {
        session_excludes.insert(id);
    }
}

/// Seed session excludes from normalized binding coin ids.
#[must_use]
pub(crate) fn session_excludes_from_binding(binding_coin_ids: &[String]) -> HashSet<String> {
    let mut out = HashSet::with_capacity(binding_coin_ids.len());
    for coin_id in binding_coin_ids {
        record_session_pin(&mut out, coin_id);
    }
    out
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
}
