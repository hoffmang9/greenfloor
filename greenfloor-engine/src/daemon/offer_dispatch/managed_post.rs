use std::path::Path;

use crate::config::{ManagerProgramConfig, MarketConfig};
use crate::cycle::PlannedAction;
use crate::error::SignerResult;
use crate::manager::{build_and_post_offer, BuildAndPostOfferRequest};
use crate::offer::request::normalize_offer_side;

pub async fn post_managed_planned_action(
    program: &ManagerProgramConfig,
    market: &MarketConfig,
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
    action: &PlannedAction,
) -> SignerResult<bool> {
    if action.size <= 0 {
        return Ok(false);
    }
    let side = normalize_offer_side(&action.side).to_string();
    let response = build_and_post_offer(BuildAndPostOfferRequest {
        program_path: program_path.to_path_buf(),
        markets_path: markets_path.to_path_buf(),
        testnet_markets_path: testnet_markets_path.map(Path::to_path_buf),
        network: program.network.clone(),
        market_id: Some(market.market_id.clone()),
        pair: None,
        size_base_units: action.size as u64,
        repeat: 1,
        publish_venue: Some(program.offer_publish_venue.clone()),
        dexie_base_url: Some(program.dexie_api_base.clone()),
        splash_base_url: Some(program.splash_api_base.clone()),
        drop_only: true,
        claim_rewards: false,
        dry_run: program.runtime_dry_run,
        compact_json: false,
        persist_results: true,
        action_side: Some(side),
    })
    .await?;
    Ok(response.exit_code == 0)
}
