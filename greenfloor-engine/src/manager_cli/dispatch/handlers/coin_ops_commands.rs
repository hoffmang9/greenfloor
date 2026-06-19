//! Split, combine, and dust dispatch handlers.

use crate::cli_util::optional_str;
use crate::error::SignerResult;

use super::super::super::coin_op_loop::{
    self, CoinSplitBehavior, CoinSplitGating, CoinSplitRequest, UntilReadyWaitMode,
};
use super::super::super::combine_market_cat_dust;
use super::super::super::commands::ManagerCommands;
use super::super::super::context::ManagerContext;
use super::super::super::util::require_market_selector;

pub async fn dispatch_coin_ops_command(
    ctx: &ManagerContext,
    command: ManagerCommands,
) -> SignerResult<i32> {
    match command {
        ManagerCommands::CoinSplit { .. } | ManagerCommands::CoinCombine { .. } => {
            dispatch_coin_mutate_command(ctx, command).await
        }
        ManagerCommands::CombineMarketCatDust { .. } => {
            dispatch_combine_market_cat_dust_command(ctx, command).await
        }
        other => Err(crate::error::SignerError::Other(format!(
            "unexpected coin-op command: {other:?}"
        ))),
    }
}

async fn dispatch_coin_mutate_command(
    ctx: &ManagerContext,
    command: ManagerCommands,
) -> SignerResult<i32> {
    match command {
        ManagerCommands::CoinSplit {
            market_id,
            pair,
            network,
            coin_id,
            amount_per_coin,
            number_of_coins,
            size_base_units,
            until_ready,
            max_iterations,
            no_wait,
            allow_lock_all_spendable,
            force_split_when_ready,
        } => {
            Box::pin(dispatch_coin_split(
                ctx,
                market_id,
                pair,
                network,
                coin_id,
                amount_per_coin,
                number_of_coins,
                size_base_units,
                max_iterations,
                CoinSplitBehavior {
                    wait: UntilReadyWaitMode {
                        until_ready,
                        no_wait,
                    },
                    gating: CoinSplitGating {
                        allow_lock_all_spendable,
                        force_split_when_ready,
                    },
                },
            ))
            .await
        }
        ManagerCommands::CoinCombine {
            market_id,
            pair,
            network,
            input_coin_count,
            asset_id,
            coin_id,
            size_base_units,
            until_ready,
            max_iterations,
            no_wait,
        } => {
            Box::pin(dispatch_coin_combine(
                ctx,
                market_id,
                pair,
                network,
                input_coin_count,
                asset_id,
                coin_id,
                size_base_units,
                until_ready,
                max_iterations,
                no_wait,
            ))
            .await
        }
        other => Err(crate::error::SignerError::Other(format!(
            "unexpected coin mutate command: {other:?}"
        ))),
    }
}

async fn dispatch_combine_market_cat_dust_command(
    ctx: &ManagerContext,
    command: ManagerCommands,
) -> SignerResult<i32> {
    match command {
        ManagerCommands::CombineMarketCatDust {
            network,
            coinset_base_url,
            launcher_id,
            launcher_id_file,
            dust_threshold_mojos,
            max_input_coins,
            max_nonce,
            cat_asset_id,
            dry_run,
            list_only,
            verify_timeout_seconds,
            verify_poll_seconds,
        } => {
            dispatch_combine_market_cat_dust(
                ctx,
                network,
                coinset_base_url,
                launcher_id,
                launcher_id_file,
                dust_threshold_mojos,
                max_input_coins,
                max_nonce,
                cat_asset_id,
                dry_run,
                list_only,
                verify_timeout_seconds,
                verify_poll_seconds,
            )
            .await
        }
        other => Err(crate::error::SignerError::Other(format!(
            "unexpected dust command: {other:?}"
        ))),
    }
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_coin_split(
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
    Box::pin(coin_op_loop::run_coin_split(CoinSplitRequest {
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
    }))
    .await
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_coin_combine(
    ctx: &ManagerContext,
    market_id: Option<String>,
    pair: Option<String>,
    network: String,
    input_coin_count: i64,
    asset_id: String,
    coin_id: Vec<String>,
    size_base_units: i64,
    until_ready: bool,
    max_iterations: i32,
    no_wait: bool,
) -> SignerResult<i32> {
    require_market_selector(market_id.as_deref(), pair.as_deref())?;
    Box::pin(coin_op_loop::run_coin_combine(
        coin_op_loop::CoinCombineRequest {
            mgr: ctx,
            network: &network,
            market_id: market_id.as_deref(),
            pair: pair.as_deref(),
            coin_ids: &coin_id,
            number_of_coins: input_coin_count,
            asset_id: optional_str(&asset_id),
            no_wait,
            size_base_units: if size_base_units > 0 {
                Some(size_base_units)
            } else {
                None
            },
            until_ready,
            max_iterations,
        },
    ))
    .await
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_combine_market_cat_dust(
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
    combine_market_cat_dust::run_combine_market_cat_dust(
        combine_market_cat_dust::CombineMarketCatDustRequest {
            mgr: ctx,
            network: optional_str(&network),
            coinset_base_url: optional_str(&coinset_base_url),
            launcher_id: optional_str(&launcher_id),
            launcher_id_file: optional_str(&launcher_id_file),
            dust_threshold_mojos,
            max_input_coins,
            max_nonce,
            cat_asset_id: optional_str(&cat_asset_id),
            verify: crate::coinset::CoinSpentVerifyConfig {
                timeout_seconds: verify_timeout_seconds,
                poll_seconds: verify_poll_seconds,
            },
            execution: combine_market_cat_dust::CombineExecutionFlags::from_flags(
                list_only, dry_run,
            ),
        },
    )
    .await
}
