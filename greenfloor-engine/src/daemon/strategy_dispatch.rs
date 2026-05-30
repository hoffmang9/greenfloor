use std::collections::BTreeMap;
use std::path::Path;

use serde_json::json;

use crate::config::{require_signer_offer_path, ManagerProgramConfig, MarketConfig};
use crate::cycle::{
    executed_sell_offer_counts_by_size, expand_planned_actions, PlannedAction,
    StrategyActionSellCountInput,
};
use crate::error::SignerResult;
use crate::manager::{build_and_post_offer, BuildAndPostOfferRequest};
use crate::offer::request::normalize_offer_side;
use crate::storage::SqliteStore;

/// Sequential managed-offer dispatch for the daemon strategy phase.
///
/// Each successful post persists offer state and a canonical ``strategy_offer_execution`` audit
/// (with ``offer_id``) via ``persist_offer_post_records``. This module intentionally does not
/// emit a second aggregate audit event.
pub struct StrategyDispatchOutput {
    pub executed_count: u64,
    pub newly_executed_sell_counts: BTreeMap<i64, i64>,
}

pub async fn execute_strategy_actions_sequential(
    store: &SqliteStore,
    program: &ManagerProgramConfig,
    market: &MarketConfig,
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
    actions: &[PlannedAction],
) -> SignerResult<StrategyDispatchOutput> {
    if require_signer_offer_path(program_path).is_err() {
        store.add_audit_event(
            "strategy_exec_skipped_no_signer",
            &json!({"market_id": market.market_id, "planned_count": actions.len()}),
            Some(&market.market_id),
        )?;
        return Ok(StrategyDispatchOutput {
            executed_count: 0,
            newly_executed_sell_counts: BTreeMap::new(),
        });
    }

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

    Ok(StrategyDispatchOutput {
        executed_count: executed,
        newly_executed_sell_counts: executed_sell_offer_counts_by_size(&action_items),
    })
}

pub fn skip_strategy_execution() -> bool {
    std::env::var_os("GREENFLOOR_TEST_SKIP_STRATEGY_EXEC").is_some()
}
