//! Explicit test fixtures for operator CLI integration tests (env opt-in only).

use serde::Deserialize;

use crate::coin_ops::SpendableCoin;

#[derive(Debug, Deserialize)]
struct TestWalletCoinRow {
    id: String,
    amount: i64,
}

pub(crate) fn test_wallet_coins_from_env() -> Option<Vec<SpendableCoin>> {
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

pub(crate) fn test_mixed_split_operation_id_from_env() -> Option<String> {
    std::env::var("GREENFLOOR_TEST_MIXED_SPLIT_OPERATION_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
