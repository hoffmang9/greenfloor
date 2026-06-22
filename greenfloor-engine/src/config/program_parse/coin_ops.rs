use serde_json::Value;

use super::super::yaml_fields::config_err;
use super::helpers::coin_ops_i64_field;
use crate::error::SignerResult;

#[allow(clippy::struct_field_names)]
pub(super) struct CoinOpsFields {
    pub coin_ops_minimum_fee_mojos: u64,
    pub coin_ops_max_operations_per_run: i64,
    pub coin_ops_max_daily_fee_budget_mojos: i64,
    pub coin_ops_split_fee_mojos: i64,
    pub coin_ops_combine_fee_mojos: i64,
}

pub(super) fn parse_coin_ops_config(
    coin_ops: Option<&serde_json::Map<String, Value>>,
) -> SignerResult<CoinOpsFields> {
    let raw_fee = coin_ops_i64_field(coin_ops, "minimum_fee_mojos", 10_000_000)?;
    if raw_fee < 0 {
        return Err(config_err("coin_ops.minimum_fee_mojos must be >= 0"));
    }
    let coin_ops_minimum_fee_mojos = u64::try_from(raw_fee)
        .map_err(|_| config_err("coin_ops.minimum_fee_mojos must fit in u64"))?;
    Ok(CoinOpsFields {
        coin_ops_minimum_fee_mojos,
        coin_ops_max_operations_per_run: coin_ops_i64_field(
            coin_ops,
            "max_operations_per_run",
            20,
        )?,
        coin_ops_max_daily_fee_budget_mojos: coin_ops_i64_field(
            coin_ops,
            "max_daily_fee_budget_mojos",
            0,
        )?,
        coin_ops_split_fee_mojos: coin_ops_i64_field(coin_ops, "split_fee_mojos", 0)?,
        coin_ops_combine_fee_mojos: coin_ops_i64_field(coin_ops, "combine_fee_mojos", 0)?,
    })
}
