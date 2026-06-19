//! Debug-build integration-test overrides (set on `CoinOpExecContext` at CLI construction).

use crate::coin_ops::SpendableCoin;

#[derive(Debug, Clone, Default)]
pub struct CoinOpTestOverrides {
    pub wallet_coins: Option<Vec<SpendableCoin>>,
    pub mixed_split_operation_id: Option<String>,
}

impl CoinOpTestOverrides {
    #[must_use]
    pub fn from_env() -> Self {
        #[cfg(debug_assertions)]
        {
            Self {
                wallet_coins: wallet_coins_from_env(),
                mixed_split_operation_id: mixed_split_operation_id_from_env(),
            }
        }
        #[cfg(not(debug_assertions))]
        {
            Self::default()
        }
    }
}

#[cfg(debug_assertions)]
fn wallet_coins_from_env() -> Option<Vec<SpendableCoin>> {
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct TestWalletCoinRow {
        id: String,
        amount: i64,
    }

    let raw = std::env::var("GREENFLOOR_TEST_WALLET_COINS_JSON").ok()?;
    let rows: Vec<TestWalletCoinRow> = serde_json::from_str(raw.trim()).ok()?;
    Some(
        rows.into_iter()
            .map(|row| SpendableCoin {
                id: row.id,
                amount: row.amount,
            })
            .collect(),
    )
}

#[cfg(debug_assertions)]
fn mixed_split_operation_id_from_env() -> Option<String> {
    std::env::var("GREENFLOOR_TEST_MIXED_SPLIT_OPERATION_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
