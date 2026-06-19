//! Offer lifecycle dispatch handlers.

use crate::error::SignerResult;
use crate::offer::operator::{
    build_and_post_offer, BuildAndPostOfferRequest, BuildAndPostRunOptions,
    BuildAndPostVenueOptions, OfferOperatorTestOverrides,
};

use super::super::super::commands::ManagerCommands;
use super::super::super::context::ManagerContext;
use super::super::super::offers::{
    run_offers_cancel_command, run_offers_reconcile_command, run_offers_status_command,
    OffersCancelCliArgs, OffersReconcileCliArgs, OffersStatusCliArgs,
};
use super::super::super::util::require_market_selector;

pub async fn dispatch_offer_command(
    ctx: &ManagerContext,
    command: ManagerCommands,
) -> SignerResult<i32> {
    match command {
        ManagerCommands::BuildAndPostOffer {
            market_id,
            pair,
            size_base_units,
            repeat,
            network,
            dexie_base_url,
            allow_take,
            claim_rewards,
            dry_run,
            venue,
            splash_base_url,
        } => {
            Box::pin(dispatch_build_and_post_offer(
                ctx,
                market_id,
                pair,
                size_base_units,
                repeat,
                network,
                dexie_base_url,
                allow_take,
                claim_rewards,
                dry_run,
                venue,
                splash_base_url,
            ))
            .await
        }
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
            Box::pin(run_offers_reconcile_command(
                ctx,
                OffersReconcileCliArgs {
                    market_id,
                    limit,
                    venue,
                },
            ))
            .await
        }
        ManagerCommands::OffersCancel {
            offer_id,
            cancel_open,
            venue,
        } => {
            Box::pin(run_offers_cancel_command(
                ctx,
                OffersCancelCliArgs {
                    offer_id,
                    cancel_open,
                    venue,
                },
            ))
            .await
        }
        other => Err(crate::error::SignerError::Other(format!(
            "unexpected offer command: {other:?}"
        ))),
    }
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_build_and_post_offer(
    ctx: &ManagerContext,
    market_id: Option<String>,
    pair: Option<String>,
    size_base_units: u64,
    repeat: u32,
    network: String,
    dexie_base_url: Option<String>,
    allow_take: bool,
    claim_rewards: bool,
    dry_run: bool,
    venue: Option<String>,
    splash_base_url: Option<String>,
) -> SignerResult<i32> {
    require_market_selector(market_id.as_deref(), pair.as_deref())?;
    let response = Box::pin(build_and_post_offer(BuildAndPostOfferRequest {
        program_path: ctx.program_config.clone(),
        markets_path: ctx.markets_config.clone(),
        testnet_markets_path: ctx.testnet_markets_path().map(std::path::Path::to_path_buf),
        network,
        market_id,
        pair,
        size_base_units,
        repeat,
        publish_venue: venue,
        dexie_base_url: dexie_base_url.or(ctx.dexie_base_url.clone()),
        splash_base_url,
        venue: BuildAndPostVenueOptions {
            drop_only: !allow_take,
            claim_rewards,
        },
        run: BuildAndPostRunOptions {
            dry_run,
            persist_results: true,
        },
        action_side: None,
        test_overrides: OfferOperatorTestOverrides::from_env(),
    }))
    .await?;
    ctx.emit_json(&response.payload)?;
    Ok(response.exit_code)
}
