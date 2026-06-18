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
            },
            WalletUnspentCoin {
                id: "coin_a".to_string(),
                name: "coin_a".to_string(),
                amount: 1000,
                state: "SETTLED".to_string(),
            },
        ];
        let spendable = wallet_coins_to_spendable(&coins, cat_id);
        assert_eq!(spendable.len(), 1);
        assert_eq!(spendable[0].id, "coin_a");
    }
}
