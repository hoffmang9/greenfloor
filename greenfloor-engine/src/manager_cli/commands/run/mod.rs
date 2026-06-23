//! Domain dispatch for native manager CLI commands.

mod build_offer;
mod cats;
mod coin_ops;
mod dust;
mod flag_groups;
mod maintenance;
mod offers;
mod setup;

use crate::async_boundary::ManagerCommandFuture;
use crate::error::SignerResult;
use crate::manager_cli::context::ManagerContext;

use super::clap::ManagerCommands;

impl ManagerCommands {
    /// Run this CLI command to completion.
    #[must_use]
    pub fn run(self, ctx: &ManagerContext) -> ManagerCommandFuture<'_> {
        Box::pin(self.run_async(ctx))
    }

    async fn run_async(self, ctx: &ManagerContext) -> SignerResult<i32> {
        match self {
            cmd @ (ManagerCommands::ConfigValidate { .. }
            | ManagerCommands::ProgramFields
            | ManagerCommands::MarketsFields
            | ManagerCommands::CatsFields
            | ManagerCommands::MaterializeMinimalProgram { .. }
            | ManagerCommands::KeysOnboard { .. }
            | ManagerCommands::Doctor
            | ManagerCommands::BootstrapHome { .. }
            | ManagerCommands::SetLogLevel { .. }) => setup::run_command(cmd, ctx),
            ManagerCommands::AuditPrune { .. } => maintenance::run_command(self, ctx),
            ManagerCommands::BuildAndPostOffer { .. } => build_offer::run_command(self, ctx).await,
            cmd @ (ManagerCommands::OffersStatus { .. }
            | ManagerCommands::OffersReconcile { .. }) => offers::run_command(cmd, ctx).await,
            cmd @ ManagerCommands::OffersCancel { .. } => {
                Box::pin(offers::run_command(cmd, ctx)).await
            }
            cmd @ (ManagerCommands::CatsAdd { .. }
            | ManagerCommands::CatsList
            | ManagerCommands::CatsDelete { .. }) => cats::run_command(cmd, ctx).await,
            cmd @ (ManagerCommands::CoinsList { .. }
            | ManagerCommands::CoinStatus { .. }
            | ManagerCommands::CoinSplit { .. }
            | ManagerCommands::CoinCombine { .. }) => coin_ops::run_command(cmd, ctx).await,
            ManagerCommands::CombineMarketCatDust { .. } => dust::run_command(self, ctx).await,
            ManagerCommands::FlagGroups { .. } => flag_groups::run_command(self, ctx),
        }
    }
}
