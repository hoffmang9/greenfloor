use std::collections::BTreeMap;
use std::path::Path;

use serde_json::json;

use crate::config::{ManagerProgramConfig, MarketConfig};
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::market_phases::MarketPhaseMetrics;
use super::reconcile_phase::ReconcilePhaseResult;
use super::offer_dispatch::{execute_strategy_actions, skip_strategy_execution};
use super::strategy_support::evaluate_strategy_actions_for_market;

pub struct StrategyPhaseResult {
    pub sell_active_counts: BTreeMap<i64, i64>,
    pub newly_executed_sell_counts: BTreeMap<i64, i64>,
}

pub async fn run_strategy_phase(
    store: &SqliteStore,
    db_path: &Path,
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
            db_path,
            program,
            market,
            network,
            program_path,
            markets_path,
            testnet_markets_path,
            &strategy_actions,
        )
        .await
        {
            Ok(output) => {
                metrics.strategy_executed_total = output.executed_count;
                newly_executed_sell_counts = output.newly_executed_sell_counts;
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
