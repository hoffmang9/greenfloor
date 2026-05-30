use std::collections::BTreeMap;
use std::path::Path;

use crate::config::{ManagerProgramConfig, MarketConfig};
use crate::cycle::{
    executed_sell_offer_counts_by_size, expand_planned_actions, PlannedAction,
    StrategyActionSellCountInput,
};
use crate::error::SignerResult;
use crate::manager::{build_and_post_offer, BuildAndPostOfferRequest};
use crate::offer::request::normalize_offer_side;
use crate::storage::SqliteStore;

use super::OfferDispatchOutput;

pub async fn execute_actions_sequential(
    store: &SqliteStore,
    program: &ManagerProgramConfig,
    market: &MarketConfig,
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
    actions: &[PlannedAction],
) -> SignerResult<OfferDispatchOutput> {
    let expanded = expand_planned_actions(actions);
    let mut executed = 0_u64;
    let mut action_items = Vec::new();

    for action in expanded {
        if action.size <= 0 {
            continue;
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
            action_side: Some(side.clone()),
        })
        .await?;

        let counts_as_executed = response.exit_code == 0;
        if counts_as_executed {
            executed += 1;
        }
        action_items.push(StrategyActionSellCountInput {
            size: action.size,
            side,
            counts_as_executed,
        });
    }

    let _ = store;
    Ok(OfferDispatchOutput {
        executed_count: executed,
        newly_executed_sell_counts: executed_sell_offer_counts_by_size(&action_items),
    })
}
