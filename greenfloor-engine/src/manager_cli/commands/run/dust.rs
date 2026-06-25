use crate::cli_util::optional_str;
use crate::coinset::CoinSpentVerifyConfig;
use crate::error::SignerResult;
use crate::manager_cli::combine_market_cat_dust::{
    self, CombineExecutionFlags, CombineMarketCatDustRequest,
};
use crate::manager_cli::context::ManagerContext;

use super::super::clap::ManagerCommands;

pub async fn run_command(command: ManagerCommands, ctx: &ManagerContext) -> SignerResult<i32> {
    let ManagerCommands::CombineMarketCatDust {
        network,
        coinset_base_url,
        launcher_id,
        launcher_id_file,
        dust_threshold_mojos,
        max_input_coins,
        max_batches,
        max_nonce,
        cat_asset_id,
        dry_run,
        list_only,
        verify_timeout_seconds,
        verify_poll_seconds,
    } = command
    else {
        unreachable!("dust::run_command called with {command:?}");
    };

    Box::pin(combine_market_cat_dust::run_combine_market_cat_dust(
        CombineMarketCatDustRequest {
            mgr: ctx,
            network: optional_str(&network),
            coinset_base_url: optional_str(&coinset_base_url),
            launcher_id: optional_str(&launcher_id),
            launcher_id_file: optional_str(&launcher_id_file),
            dust_threshold_mojos,
            max_input_coins,
            max_batches,
            max_nonce,
            cat_asset_id: optional_str(&cat_asset_id),
            verify: CoinSpentVerifyConfig {
                timeout_seconds: verify_timeout_seconds,
                poll_seconds: verify_poll_seconds,
            },
            execution: CombineExecutionFlags::from_flags(list_only, dry_run),
        },
    ))
    .await
}
