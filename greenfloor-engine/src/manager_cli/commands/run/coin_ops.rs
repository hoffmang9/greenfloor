use crate::cli_util::optional_str;
use crate::error::SignerResult;
use crate::manager_cli::coin_op_loop::{
    self, CoinCombineBehavior, CoinCombineRequest, CoinSplitBehavior, CoinSplitRequest,
};
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::util::require_market_selector;

use super::super::clap::ManagerCommands;

#[must_use]
fn optional_positive_size(size_base_units: i64) -> Option<i64> {
    (size_base_units > 0).then_some(size_base_units)
}

#[allow(clippy::too_many_lines)]
pub async fn run_command(command: ManagerCommands, ctx: &ManagerContext) -> SignerResult<i32> {
    match command {
        ManagerCommands::CoinsList {
            market_id,
            pair,
            network,
            asset,
            vault_id,
            cat_id,
        } => {
            coin_op_loop::run_coins_list(
                ctx,
                &network,
                market_id.as_deref(),
                pair.as_deref(),
                optional_str(&asset),
                optional_str(&vault_id),
                optional_str(&cat_id),
            )
            .await
        }
        ManagerCommands::CoinStatus {
            market_id,
            pair,
            network,
            asset,
            vault_id,
            cat_id,
        } => {
            coin_op_loop::run_coin_status(
                ctx,
                &network,
                market_id.as_deref(),
                pair.as_deref(),
                optional_str(&asset),
                optional_str(&vault_id),
                optional_str(&cat_id),
            )
            .await
        }
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
            require_market_selector(market_id.as_deref(), pair.as_deref())?;
            coin_op_loop::run_coin_split(CoinSplitRequest {
                mgr: ctx,
                network: &network,
                market_id: market_id.as_deref(),
                pair: pair.as_deref(),
                coin_ids: &coin_id,
                amount_per_coin,
                number_of_coins,
                behavior: CoinSplitBehavior::from_cli(
                    until_ready,
                    no_wait,
                    allow_lock_all_spendable,
                    force_split_when_ready,
                ),
                size_base_units: optional_positive_size(size_base_units),
                max_iterations,
            })
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
            require_market_selector(market_id.as_deref(), pair.as_deref())?;
            coin_op_loop::run_coin_combine(CoinCombineRequest {
                mgr: ctx,
                network: &network,
                market_id: market_id.as_deref(),
                pair: pair.as_deref(),
                coin_ids: &coin_id,
                number_of_coins: input_coin_count,
                asset_id: optional_str(&asset_id),
                behavior: CoinCombineBehavior::from_cli(until_ready, no_wait),
                size_base_units: optional_positive_size(size_base_units),
                max_iterations,
            })
            .await
        }
        other => unreachable!("coin_ops::run_command called with {other:?}"),
    }
}
