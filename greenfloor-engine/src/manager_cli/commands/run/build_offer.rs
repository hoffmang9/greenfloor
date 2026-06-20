use crate::error::SignerResult;
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::util::require_market_selector;
use crate::offer::operator::{
    build_and_post_offer, BuildAndPostOfferRequest, BuildAndPostRunOptions,
    BuildAndPostVenueOptions, OfferOperatorTestOverrides,
};

use super::super::clap::ManagerCommands;

pub async fn run_command(command: ManagerCommands, ctx: &ManagerContext) -> SignerResult<i32> {
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
        unreachable!("build_offer::run_command called with {command:?}");
    };

    require_market_selector(market_id.as_deref(), pair.as_deref())?;
    let response = build_and_post_offer(BuildAndPostOfferRequest {
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
        test_overrides: OfferOperatorTestOverrides::default(),
    })
    .await?;
    ctx.emit_json(&response.payload)?;
    Ok(response.exit_code)
}
