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
