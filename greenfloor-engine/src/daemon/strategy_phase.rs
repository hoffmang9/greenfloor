use std::collections::BTreeMap;
use std::path::Path;

use serde_json::json;

use crate::config::{require_signer_offer_path, MarketConfig, ManagerProgramConfig};
use crate::cycle::{
    executed_sell_offer_counts_by_size, PlannedAction,
    StrategyActionSellCountInput,
};
use crate::error::SignerResult;
use crate::manager::{build_and_post_offer, BuildAndPostOfferRequest};
use crate::offer::request::normalize_offer_side;
use crate::storage::SqliteStore;

use super::market_phases::MarketPhaseMetrics;
use super::reconcile_phase::ReconcilePhaseResult;
use super::strategy_support::evaluate_strategy_actions_for_market;

pub struct StrategyPhaseResult {
    pub sell_active_counts: BTreeMap<i64, i64>,
    pub newly_executed_sell_counts: BTreeMap<i64, i64>,
}

pub async fn run_strategy_phase(
    store: &SqliteStore,
    program: &ManagerProgramConfig,
    market: &MarketConfig,
    network: &str,
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
    reconcile: &ReconcilePhaseResult,
    xch_price_usd: Option<f64>,
    metrics: &mut MarketPhaseMetrics,
) -> SignerResult<StrategyPhaseResult> {
    let (strategy_actions, sell_active_counts) = evaluate_strategy_actions_for_market(
        store,
        market,
        network,
        &reconcile.dexie_size_by_offer_id,
        xch_price_usd,
    )?;
    metrics.strategy_planned_total = strategy_actions.len() as u64;

    store.add_audit_event(
        "strategy_actions_planned",
        &json!({
            "market_id": market.market_id,
            "xch_price_usd": xch_price_usd,
            "action_count": strategy_actions.len(),
        }),
        Some(&market.market_id),
    )?;

    let mut newly_executed_sell_counts = BTreeMap::new();
    if !strategy_actions.is_empty() && !skip_strategy_execution() {
        match execute_strategy_actions(
            store,
            program,
            market,
            program_path,
            markets_path,
            testnet_markets_path,
            &strategy_actions,
        )
        .await
        {
            Ok((executed, action_items)) => {
                metrics.strategy_executed_total = executed;
                newly_executed_sell_counts = executed_sell_offer_counts_by_size(&action_items);
                store.add_audit_event(
                    "strategy_offer_execution",
                    &json!({
                        "market_id": market.market_id,
                        "planned_count": strategy_actions.len(),
                        "executed_count": executed,
                        "items": action_items.iter().map(strategy_item_json).collect::<Vec<_>>(),
                    }),
                    Some(&market.market_id),
                )?;
            }
            Err(err) => {
                metrics.cycle_error_count += 1;
                store.add_audit_event(
                    "strategy_offer_execution_error",
                    &json!({"market_id": market.market_id, "error": err.to_string()}),
                    Some(&market.market_id),
                )?;
            }
        }
    }

    Ok(StrategyPhaseResult {
        sell_active_counts,
        newly_executed_sell_counts,
    })
}

fn strategy_item_json(item: &StrategyActionSellCountInput) -> serde_json::Value {
    json!({
        "size": item.size,
        "side": item.side,
        "counts_as_executed": item.counts_as_executed,
    })
}

async fn execute_strategy_actions(
    store: &SqliteStore,
    program: &ManagerProgramConfig,
    market: &MarketConfig,
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
    actions: &[PlannedAction],
) -> SignerResult<(u64, Vec<StrategyActionSellCountInput>)> {
    if require_signer_offer_path(program_path).is_err() {
        store.add_audit_event(
            "strategy_exec_skipped_no_signer",
            &json!({"market_id": market.market_id, "planned_count": actions.len()}),
            Some(&market.market_id),
        )?;
        return Ok((0, Vec::new()));
    }

    let mut executed = 0_u64;
    let mut action_items = Vec::new();
    for action in actions {
        if action.repeat <= 0 || action.size <= 0 {
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
            repeat: action.repeat as u32,
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
            executed += action.repeat as u64;
        }
        for _ in 0..action.repeat.max(0) {
            action_items.push(StrategyActionSellCountInput {
                size: action.size,
                side: side.clone(),
                counts_as_executed,
            });
        }
    }
    Ok((executed, action_items))
}

fn skip_strategy_execution() -> bool {
    std::env::var_os("GREENFLOOR_TEST_SKIP_STRATEGY_EXEC").is_some()
}
