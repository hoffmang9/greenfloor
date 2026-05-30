use std::collections::HashMap;

use crate::coin_ops::{coin_op_min_amount_mojos, SpendableCoin};
use crate::coinset::WalletUnspentCoin;

pub(crate) fn wallet_coins_to_spendable(
    coins: &[WalletUnspentCoin],
    canonical_asset_id: &str,
) -> Vec<SpendableCoin> {
    let min_amount = coin_op_min_amount_mojos(canonical_asset_id);
    coins
        .iter()
        .filter(|coin| i64::try_from(coin.amount).unwrap_or(0) >= min_amount)
        .map(|coin| SpendableCoin {
            id: coin.id.clone(),
            amount: i64::try_from(coin.amount).unwrap_or(i64::MAX),
        })
        .collect()
}

pub(crate) fn combine_output_amounts(total: i64, output_count: usize) -> Vec<u64> {
    let output_count = output_count.max(1);
    let base = total.div_euclid(output_count as i64);
    let remainder = total.rem_euclid(output_count as i64);
    let mut output_amounts = vec![base.max(0) as u64; output_count];
    if let Some(last) = output_amounts.last_mut() {
        *last = last.saturating_add(remainder.max(0) as u64);
    }
    output_amounts
}

pub(crate) fn total_for_coin_ids(spendable: &[SpendableCoin], coin_ids: &[String]) -> i64 {
    let amount_by_id: HashMap<String, i64> = spendable
        .iter()
        .map(|coin| (coin.id.to_ascii_lowercase(), coin.amount))
        .collect();
    coin_ids
        .iter()
        .map(|coin_id| {
            amount_by_id
                .get(&coin_id.to_ascii_lowercase())
                .copied()
                .unwrap_or(0)
        })
        .sum()
}
