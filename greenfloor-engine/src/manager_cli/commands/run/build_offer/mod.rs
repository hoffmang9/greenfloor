use crate::error::SignerResult;
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::util::require_market_selector;
use crate::offer::operator::{
    build_and_post_offer, BuildAndPostOfferRequestParts, BuildAndPostRunOptions,
    BuildAndPostVenueOptions,
};

#[cfg(test)]
use crate::offer::operator::BuildOfferTestOverrides;

use super::super::clap::ManagerCommands;

pub(crate) fn build_and_post_request(
    command: &ManagerCommands,
    ctx: &ManagerContext,
) -> SignerResult<crate::offer::operator::BuildAndPostOfferRequest> {
    let ManagerCommands::BuildAndPostOffer {
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
    } = command
    else {
        unreachable!("build_offer::build_and_post_request called with {command:?}");
    };

    require_market_selector(market_id.as_deref(), pair.as_deref())?;
    Ok(
        crate::offer::operator::BuildAndPostOfferRequest::from_parts(
            BuildAndPostOfferRequestParts {
                program_path: ctx.program_config.clone(),
                markets_path: ctx.markets_config.clone(),
                testnet_markets_path: ctx.testnet_markets_path().map(std::path::Path::to_path_buf),
                cats_path: Some(ctx.cats_config.clone()),
                network: network.clone(),
                market_id: market_id.clone(),
                pair: pair.clone(),
                size_base_units: *size_base_units,
                repeat: *repeat,
                publish_venue: venue.clone(),
                dexie_base_url: dexie_base_url.clone().or(ctx.dexie_base_url.clone()),
                splash_base_url: splash_base_url.clone(),
                venue: BuildAndPostVenueOptions {
                    drop_only: !allow_take,
                    claim_rewards: *claim_rewards,
                },
                run: BuildAndPostRunOptions {
                    dry_run: *dry_run,
                    persist_results: true,
                },
                action_side: None,
            },
        ),
    )
}

pub async fn run_command(command: ManagerCommands, ctx: &ManagerContext) -> SignerResult<i32> {
    let request = build_and_post_request(&command, ctx)?;
    let response = build_and_post_offer(request).await?;
    ctx.emit_json(&response.payload)?;
    Ok(response.exit_code)
}

#[cfg(test)]
pub(crate) async fn run_command_with_test_overrides(
    command: ManagerCommands,
    ctx: &ManagerContext,
    test_overrides: BuildOfferTestOverrides,
) -> SignerResult<i32> {
    let mut request = build_and_post_request(&command, ctx)?;
    request.test_overrides = test_overrides;
    let response = build_and_post_offer(request).await?;
    ctx.emit_json(&response.payload)?;
    Ok(response.exit_code)
}

#[cfg(test)]
mod tests;
