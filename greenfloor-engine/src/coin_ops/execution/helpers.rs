use std::collections::HashSet;

use crate::coin_ops::{coin_op_min_amount_mojos, SpendableCoin};
use crate::coinset::WalletUnspentCoin;
use crate::hex::normalize_hex_id;

pub(crate) fn wallet_coins_to_spendable(
    coins: &[WalletUnspentCoin],
    canonical_asset_id: &str,
) -> Vec<SpendableCoin> {
    let min_amount = coin_op_min_amount_mojos(canonical_asset_id);
    coins
        .iter()
        .filter_map(|coin| {
            let amount = i64::try_from(coin.amount).ok()?;
            (amount >= min_amount).then_some(SpendableCoin {
                id: coin.id.clone(),
                amount,
                puzzle_hash: coin.puzzle_hash.clone(),
            })
        })
        .collect()
}

/// Drop durable maker watches (coin id or on-chain p2) from a spendable set.
///
/// When `watched_p2s` is non-empty, coins with an empty `puzzle_hash` are excluded
/// (fail closed) until the wallet path populates on-chain puzzle hashes.
pub(crate) fn exclude_watched_spendable(
    coins: impl IntoIterator<Item = SpendableCoin>,
    watched_coin_ids: &HashSet<String>,
    watched_p2s: &HashSet<String>,
) -> Vec<SpendableCoin> {
    coins
        .into_iter()
        .filter(|coin| {
            let id = coin.id.to_ascii_lowercase();
            if watched_coin_ids.contains(&id) {
                return false;
            }
            if watched_p2s.is_empty() {
                return true;
            }
            let p2 = normalize_hex_id(&coin.puzzle_hash);
            !p2.is_empty() && !watched_p2s.contains(&p2)
        })
        .collect()
}

/// Coin ids from `coin_ids` that are durable maker watches (`kind='coin'`).
#[must_use]
pub(crate) fn watched_maker_coin_ids<'a>(
    coin_ids: &'a [String],
    watched_coin_ids: &HashSet<String>,
) -> Vec<&'a str> {
    coin_ids
        .iter()
        .filter(|id| watched_coin_ids.contains(&id.to_ascii_lowercase()))
        .map(String::as_str)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallet_coins_to_spendable_filters_cat_dust() {
        let cat_id = "0000000000000000000000000000000000000000000000000000000000000001";
        let coins = vec![
            WalletUnspentCoin {
                id: "dust".to_string(),
                name: "dust".to_string(),
                amount: 500,
                state: "SETTLED".to_string(),
                puzzle_hash: String::new(),
            },
            WalletUnspentCoin {
                id: "coin_a".to_string(),
                name: "coin_a".to_string(),
                amount: 1000,
                state: "SETTLED".to_string(),
                puzzle_hash: String::new(),
            },
        ];
        let spendable = wallet_coins_to_spendable(&coins, cat_id);
        assert_eq!(spendable.len(), 1);
        assert_eq!(spendable[0].id, "coin_a");
    }

    #[test]
    fn exclude_watched_spendable_drops_coin_id_and_p2_matches() {
        let watched_coins = HashSet::from(["aa".repeat(32)]);
        let watched_p2s = HashSet::from(["bb".repeat(32)]);
        let coins = vec![
            SpendableCoin::new("aa".repeat(32), 1000),
            SpendableCoin::with_puzzle_hash("cc".repeat(32), 2000, "bb".repeat(32)),
            SpendableCoin::with_puzzle_hash("dd".repeat(32), 3000, "ee".repeat(32)),
            SpendableCoin::new("ff".repeat(32), 4000),
        ];
        let kept = exclude_watched_spendable(coins, &watched_coins, &watched_p2s);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "dd".repeat(32));
    }

    #[test]
    fn exclude_watched_spendable_empty_p2_fails_closed_when_p2_watches_exist() {
        let watched_p2s = HashSet::from(["bb".repeat(32)]);
        let coins = vec![SpendableCoin::new("ff".repeat(32), 4000)];
        let kept = exclude_watched_spendable(coins, &HashSet::default(), &watched_p2s);
        assert!(kept.is_empty());
    }

    #[test]
    fn watched_maker_coin_ids_matches_case_insensitively() {
        let watched = HashSet::from(["aa".repeat(32)]);
        let maker = "AA".repeat(32);
        let free = "bb".repeat(32);
        let ids = vec![maker.clone(), free];
        assert_eq!(watched_maker_coin_ids(&ids, &watched), vec![maker.as_str()]);
    }
}
