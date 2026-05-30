use std::collections::BTreeMap;

use crate::config::{ManagerProgramConfig, MarketConfig};
use crate::cycle::{
    executed_sell_offer_counts_by_size, PlannedAction, StrategyActionSellCountInput,
};
use crate::error::SignerResult;
use crate::offer::request::normalize_offer_side;

use crate::daemon::config_paths::DaemonConfigPaths;
use super::managed_post::post_managed_planned_action;
use super::OfferDispatchOutput;

pub async fn execute_actions_sequential(
    program: &ManagerProgramConfig,
    paths: &DaemonConfigPaths,
    market: &MarketConfig,
    expanded: &[PlannedAction],
) -> SignerResult<OfferDispatchOutput> {
    let mut executed = 0_u64;
    let mut action_items = Vec::new();

    for action in expanded {
        let side = normalize_offer_side(&action.side).to_string();
        let counts_as_executed =
            post_managed_planned_action(program, paths, market, action).await?;
        if counts_as_executed {
            executed += 1;
        }
        action_items.push(StrategyActionSellCountInput {
            size: action.size,
            side,
            counts_as_executed,
        });
    }

    Ok(OfferDispatchOutput {
        executed_count: executed,
        newly_executed_sell_counts: executed_sell_offer_counts_by_size(&action_items),
    })
}
