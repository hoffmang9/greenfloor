use std::collections::BTreeMap;

use serde_json::json;

use crate::config::MarketConfig;
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use crate::cycle::MarketCycleResultState;

use super::market_context::MarketCycleContext;
use super::offer_dispatch::execute_strategy_actions;
use super::strategy_support::evaluate_strategy_actions_for_market;

pub struct StrategyPhaseResult {
    pub sell_active_counts: BTreeMap<i64, i64>,
    pub newly_executed_sell_counts: BTreeMap<i64, i64>,
}

pub async fn run_strategy_phase(
    store: &SqliteStore,
    ctx: &MarketCycleContext<'_>,
    market: &MarketConfig,
    state: &mut MarketCycleResultState,
) -> SignerResult<StrategyPhaseResult> {
    let (strategy_actions, sell_active_counts) = evaluate_strategy_actions_for_market(
        store,
        market,
        &ctx.resources.network,
        &ctx.reconcile.dexie_size_by_offer_id,
        ctx.dispatch.xch_price_usd,
    )?;
    state.merge_strategy_execution(strategy_actions.len() as i64, 0);

    store.add_audit_event(
        "strategy_actions_planned",
        &json!({
            "market_id": market.market_id,
            "xch_price_usd": ctx.dispatch.xch_price_usd,
            "action_count": strategy_actions.len(),
        }),
        Some(&market.market_id),
    )?;

    let mut newly_executed_sell_counts = BTreeMap::new();
    if !strategy_actions.is_empty() && !ctx.dispatch.test_controls.skip_strategy_execution {
        match execute_strategy_actions(
            store,
            &ctx.dispatch.db_path,
            &ctx.resources.program,
            &ctx.resources.paths,
            market,
            &ctx.resources.network,
            &strategy_actions,
            ctx.resources.signer_offer_path_configured,
        )
        .await
        {
            Ok(output) => {
                state.merge_strategy_execution(0, output.executed_count as i64);
                newly_executed_sell_counts = output.newly_executed_sell_counts;
            }
            Err(err) => {
                state.record_phase_error();
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
