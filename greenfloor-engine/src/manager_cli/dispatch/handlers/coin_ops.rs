//! Coin split/combine/dust dispatch helpers.

use crate::cli_util::optional_str;
use crate::coinset::CoinSpentVerifyConfig;
use crate::error::SignerResult;
use crate::manager_cli::coin_op_loop::{
    self, CoinSplitBehavior, CoinSplitRequest, UntilReadyWaitMode,
};
use crate::manager_cli::combine_market_cat_dust::{
    self, CombineExecutionFlags, CombineMarketCatDustRequest,
};
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::util::require_market_selector;

#[allow(clippy::too_many_arguments)]
pub async fn run_coin_split(
    ctx: &ManagerContext,
    market_id: Option<String>,
    pair: Option<String>,
    network: String,
    coin_id: Vec<String>,
    amount_per_coin: i64,
    number_of_coins: i64,
    size_base_units: i64,
    max_iterations: i32,
    behavior: CoinSplitBehavior,
) -> SignerResult<i32> {
    require_market_selector(market_id.as_deref(), pair.as_deref())?;
    coin_op_loop::run_coin_split(CoinSplitRequest {
        mgr: ctx,
        network: &network,
        market_id: market_id.as_deref(),
        pair: pair.as_deref(),
        coin_ids: &coin_id,
        amount_per_coin,
        number_of_coins,
        behavior,
        size_base_units: if size_base_units > 0 {
            Some(size_base_units)
        } else {
            None
        },
        max_iterations,
    })
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_coin_combine(
    ctx: &ManagerContext,
    market_id: Option<String>,
    pair: Option<String>,
    network: String,
    input_coin_count: i64,
    asset_id: String,
    coin_id: Vec<String>,
    size_base_units: i64,
    wait: UntilReadyWaitMode,
    max_iterations: i32,
) -> SignerResult<i32> {
    require_market_selector(market_id.as_deref(), pair.as_deref())?;
    coin_op_loop::run_coin_combine(coin_op_loop::CoinCombineRequest {
        mgr: ctx,
        network: &network,
        market_id: market_id.as_deref(),
        pair: pair.as_deref(),
        coin_ids: &coin_id,
        number_of_coins: input_coin_count,
        asset_id: optional_str(&asset_id),
        wait,
        size_base_units: if size_base_units > 0 {
            Some(size_base_units)
        } else {
            None
        },
        max_iterations,
    })
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_combine_market_cat_dust(
    ctx: &ManagerContext,
    network: String,
    coinset_base_url: String,
    launcher_id: String,
    launcher_id_file: String,
    dust_threshold_mojos: u64,
    max_input_coins: usize,
    max_nonce: u32,
    cat_asset_id: String,
    dry_run: bool,
    list_only: bool,
    verify_timeout_seconds: u64,
    verify_poll_seconds: u64,
) -> SignerResult<i32> {
    combine_market_cat_dust::run_combine_market_cat_dust(CombineMarketCatDustRequest {
        mgr: ctx,
        network: optional_str(&network),
        coinset_base_url: optional_str(&coinset_base_url),
        launcher_id: optional_str(&launcher_id),
        launcher_id_file: optional_str(&launcher_id_file),
        dust_threshold_mojos,
        max_input_coins,
        max_nonce,
        cat_asset_id: optional_str(&cat_asset_id),
        verify: CoinSpentVerifyConfig {
            timeout_seconds: verify_timeout_seconds,
            poll_seconds: verify_poll_seconds,
        },
        execution: CombineExecutionFlags::from_flags(list_only, dry_run),
    })
    .await
}
