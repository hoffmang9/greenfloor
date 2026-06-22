use crate::error::SignerResult;
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::offers::{
    run_offers_cancel_command, run_offers_reconcile_command, run_offers_status_command,
    OffersCancelCliArgs, OffersReconcileCliArgs, OffersStatusCliArgs,
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
        other => unreachable!("offers::run_command called with {other:?}"),
    }
}
