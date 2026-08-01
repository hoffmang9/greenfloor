use crate::error::SignerResult;
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::offers::{
    run_offers_cancel_command, run_offers_orphan_presplit_command,
    run_offers_reclaim_presplit_command, run_offers_reconcile_command, run_offers_status_command,
    OffersCancelCliArgs, OffersOrphanPresplitCliArgs, OffersReclaimPresplitCliArgs,
    OffersReconcileCliArgs, OffersStatusCliArgs,
};

use super::super::clap::ManagerCommands;

pub async fn run_command(command: ManagerCommands, ctx: &ManagerContext) -> SignerResult<i32> {
    match command {
        ManagerCommands::OffersStatus {
            market_id,
            limit,
            events_limit,
        } => run_offers_status_command(
            ctx,
            &OffersStatusCliArgs {
                market_id,
                limit,
                events_limit,
            },
        ),
        ManagerCommands::OffersReconcile {
            market_id,
            limit,
            venue,
        } => {
            run_offers_reconcile_command(
                ctx,
                OffersReconcileCliArgs {
                    market_id,
                    limit,
                    venue,
                },
            )
            .await
        }
        ManagerCommands::OffersCancel {
            offer_id,
            offer_file,
            market_id,
            cancel_open,
            venue,
        } => {
            Box::pin(run_offers_cancel_command(
                ctx,
                OffersCancelCliArgs {
                    offer_id,
                    offer_file,
                    market_id,
                    cancel_open,
                    venue,
                },
            ))
            .await
        }
        ManagerCommands::OffersReclaimPresplit {
            coin_id,
            fixed_delegated_puzzle_hash,
            dry_run,
        } => {
            Box::pin(run_offers_reclaim_presplit_command(
                ctx,
                OffersReclaimPresplitCliArgs {
                    coin_id,
                    fixed_delegated_puzzle_hash,
                    dry_run,
                },
            ))
            .await
        }
        ManagerCommands::OffersOrphanPresplit {
            asset,
            market_id,
            network,
            coinset_base_url,
            launcher_id,
            launcher_id_file,
            max_nonce,
            start_height,
            reclaim,
            dry_run,
            no_wait,
        } => {
            Box::pin(run_offers_orphan_presplit_command(
                ctx,
                OffersOrphanPresplitCliArgs {
                    asset,
                    market_id,
                    network,
                    coinset_base_url,
                    launcher_id,
                    launcher_id_file,
                    max_nonce,
                    start_height,
                    reclaim,
                    dry_run,
                    no_wait,
                },
            ))
            .await
        }
        other => unreachable!("offers::run_command called with {other:?}"),
    }
}
