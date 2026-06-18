use crate::config::{ManagerProgramConfig, MarketConfig};
use crate::cycle::PlannedAction;
use crate::error::SignerResult;
use crate::offer::operator::{
    build_and_post_offer, BuildAndPostOfferRequest, OfferOperatorTestOverrides,
};
use crate::offer::request::normalize_offer_side;

use crate::daemon::cycle_paths::DaemonCyclePaths;

pub async fn post_managed_planned_action(
    program: &ManagerProgramConfig,
    paths: &DaemonCyclePaths,
    market: &MarketConfig,
    action: &PlannedAction,
) -> SignerResult<bool> {
    #[cfg(test)]
    if let Some(result) = super::test_hooks::managed_post_test_override() {
        return result;
    }
    if action.size <= 0 {
        return Ok(false);
    }
    let side = normalize_offer_side(&action.side).to_string();
    let response = build_and_post_offer(BuildAndPostOfferRequest {
        program_path: paths.program_path.clone(),
        markets_path: paths.markets_path.clone(),
        testnet_markets_path: paths.testnet_markets_path.clone(),
        network: program.network.clone(),
        market_id: Some(market.market_id.clone()),
        pair: None,
        size_base_units: crate::config::non_negative_i64_to_u64(action.size, "action.size")?,
        repeat: 1,
        publish_venue: Some(program.offer_publish_venue.clone()),
        dexie_base_url: Some(program.dexie_api_base.clone()),
        splash_base_url: Some(program.splash_api_base.clone()),
        drop_only: true,
        claim_rewards: false,
        dry_run: program.runtime_dry_run,
        persist_results: true,
        action_side: Some(side),
        test_overrides: OfferOperatorTestOverrides::default(),
    })
    .await?;
    Ok(response.exit_code == 0)
}
