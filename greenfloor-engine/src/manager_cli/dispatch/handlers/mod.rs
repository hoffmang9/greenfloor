//! Per-command handlers for the native manager CLI dispatch table.

mod cats_commands;
mod coin_ops_commands;
mod coin_query_commands;
mod offers;
mod setup;

use crate::error::SignerResult;

use super::super::commands::ManagerCommands;
use super::super::context::ManagerContext;

pub async fn dispatch_manager_command(
    ctx: &ManagerContext,
    command: ManagerCommands,
) -> SignerResult<i32> {
    match command {
        ManagerCommands::ConfigValidate { .. }
        | ManagerCommands::ProgramFields
        | ManagerCommands::MarketsFields
        | ManagerCommands::CatsFields
        | ManagerCommands::MaterializeMinimalProgram { .. }
        | ManagerCommands::KeysOnboard { .. }
        | ManagerCommands::Doctor
        | ManagerCommands::BootstrapHome { .. }
        | ManagerCommands::SetLogLevel { .. } => setup::dispatch_setup_command(ctx, command),
        ManagerCommands::BuildAndPostOffer { .. }
        | ManagerCommands::OffersStatus { .. }
        | ManagerCommands::OffersReconcile { .. }
        | ManagerCommands::OffersCancel { .. } => {
            offers::dispatch_offer_command(ctx, command).await
        }
        ManagerCommands::CatsAdd { .. }
        | ManagerCommands::CatsList
        | ManagerCommands::CatsDelete { .. } => {
            cats_commands::dispatch_cats_command(ctx, command).await
        }
        ManagerCommands::CoinsList { .. } | ManagerCommands::CoinStatus { .. } => {
            coin_query_commands::dispatch_coin_query_command(ctx, command).await
        }
        ManagerCommands::CoinSplit { .. }
        | ManagerCommands::CoinCombine { .. }
        | ManagerCommands::CombineMarketCatDust { .. } => {
            coin_ops_commands::dispatch_coin_ops_command(ctx, command).await
        }
        ManagerCommands::FlagGroups { subcommand } => {
            let payload = super::super::flag_groups::emit_flag_groups(&subcommand)?;
            ctx.emit_json(&payload)?;
            Ok(0)
        }
    }
}
