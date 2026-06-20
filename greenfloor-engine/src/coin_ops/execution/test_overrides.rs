//! Explicit test overrides for coin-op execution (injected via `CoinOpExecContext`).

use crate::coin_ops::SpendableCoin;

#[derive(Debug, Clone, Default)]
pub struct CoinOpTestOverrides {
    pub wallet_coins: Option<Vec<SpendableCoin>>,
    pub mixed_split_operation_id: Option<String>,
}

impl CoinOpTestOverrides {
    pub(crate) fn wallet_coins_override(&self) -> Option<&[SpendableCoin]> {
        self.wallet_coins.as_deref()
    }

    pub(crate) fn mixed_split_operation_id_override(&self) -> Option<&str> {
        self.mixed_split_operation_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}
