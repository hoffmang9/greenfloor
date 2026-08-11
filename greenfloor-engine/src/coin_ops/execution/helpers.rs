use std::collections::HashSet;

use crate::coin_ops::{coin_op_min_amount_mojos, SpendableCoin};
use crate::coinset::WalletUnspentCoin;

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

/// Drop durable maker **coin-id** watches from a spendable set.
///
/// Coin-ops does not exclude by `kind='p2'` watches: Direct maker puzzle hashes
/// are shared vault inventory hashes and would lock the entire asset balance.
/// Offer locks are coin-id watches registered at post (ADR 0019).
pub(crate) fn exclude_watched_spendable(
    coins: &[SpendableCoin],
    watched_coin_ids: &HashSet<String>,
) -> Vec<SpendableCoin> {
    coins
        .iter()
        .filter(|coin| !watched_coin_ids.contains(&coin.id.to_ascii_lowercase()))
        .cloned()
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
    fn exclude_watched_spendable_drops_coin_id_matches_only() {
        let watched_coins = HashSet::from(["aa".repeat(32)]);
        let coins = vec![
            SpendableCoin::new("aa".repeat(32), 1000),
            SpendableCoin::with_puzzle_hash("cc".repeat(32), 2000, "bb".repeat(32)),
            SpendableCoin::with_puzzle_hash("dd".repeat(32), 3000, "ee".repeat(32)),
            SpendableCoin::new("ff".repeat(32), 4000),
        ];
        let kept = exclude_watched_spendable(&coins, &watched_coins);
        assert_eq!(kept.len(), 3);
        assert!(kept.iter().all(|coin| coin.id != "aa".repeat(32)));
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
