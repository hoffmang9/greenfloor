//! Build-and-post offer dispatch helper.

use crate::error::SignerResult;
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::util::require_market_selector;
use crate::offer::operator::{
    build_and_post_offer, BuildAndPostOfferRequest, BuildAndPostRunOptions,
    BuildAndPostVenueOptions, OfferOperatorTestOverrides,
};

#[allow(clippy::too_many_arguments)]
pub async fn run_build_and_post_offer(
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
        test_overrides: OfferOperatorTestOverrides::from_env(),
    })
    .await?;
    ctx.emit_json(&response.payload)?;
    Ok(response.exit_code)
}
