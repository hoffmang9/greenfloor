use std::collections::BTreeMap;

use serde_json::json;
use tracing::Level;

use crate::coin_ops::{
    effective_sell_bucket_counts_for_coin_ops, partition_plans_by_budget, plan_coin_ops,
    projected_coin_ops_fee_mojos, BucketSpec, CoinOpPlan,
};
use crate::config::{
    signer_execution_skip_reason, LadderEntry, ManagerProgramConfig, MarketConfig,
};
use crate::error::SignerResult;
use crate::operator_log::{
    LogContext, COIN_OPS_EXECUTED, COIN_OPS_INVALID_LADDER_MATH, COIN_OPS_NO_PLANS,
    COIN_OPS_PARTIAL_OR_SKIPPED_FEE_BUDGET, COIN_OPS_PLAN, COIN_OPS_SKIPPED_FEE_BUDGET,
};
use crate::storage::SqliteStore;

use super::coin_ops_execution::{
    execute_managed_coin_op_plans, persist_coin_op_execution, CoinOpExecItem, CoinOpExecutionResult,
};
use super::market_context::MarketCycleContext;

mod ladder;

use ladder::build_valid_sell_ladder;

#[cfg(test)]
pub(crate) mod harness;

#[cfg(test)]
mod tests;

struct CoinOpsPlanningResult {
    plans: Vec<CoinOpPlan>,
    projected_fee: i64,
    spent_today: i64,
    executable_plans: Vec<CoinOpPlan>,
    overflow_plans: Vec<CoinOpPlan>,
}

fn plan_coin_ops_for_market(
    store: &SqliteStore,
    program: &ManagerProgramConfig,
    market: &MarketConfig,
    valid_ladder: &[LadderEntry],
    wallet_bucket_counts: &BTreeMap<i64, i64>,
    active_counts: &BTreeMap<i64, i64>,
    newly_executed_counts: &BTreeMap<i64, i64>,
) -> SignerResult<Option<CoinOpsPlanningResult>> {
    let bucket_counts = effective_sell_bucket_counts_for_coin_ops(
        &valid_ladder
            .iter()
            .map(|entry| crate::coin_ops::LadderTargetRow {
                size_base_units: entry.size_base_units,
                target_count: entry.target_count,
            })
            .collect::<Vec<_>>(),
        wallet_bucket_counts,
        Some(active_counts),
        Some(newly_executed_counts),
    );
    let buckets: Vec<BucketSpec> = valid_ladder
        .iter()
        .map(|entry| BucketSpec {
            size_base_units: entry.size_base_units,
            target_count: entry.target_count,
            split_buffer_count: entry.split_buffer_count,
            combine_when_excess_factor: entry.combine_when_excess_factor,
            current_count: *bucket_counts.get(&entry.size_base_units).unwrap_or(&0),
        })
        .collect();
    let planning = plan_coin_ops(
        &buckets,
        program.coin_ops_max_operations_per_run,
        program.coin_ops_max_daily_fee_budget_mojos,
        program.coin_ops_split_fee_mojos,
        program.coin_ops_combine_fee_mojos,
    );
    if !planning.invalid_ladder_math_sizes.is_empty() {
        LogContext::MARKET_CYCLE.dual_audit(
            store,
            Level::WARN,
            "coin ops invalid ladder math",
            COIN_OPS_INVALID_LADDER_MATH,
            &json!({
                "market_id": market.market_id,
                "invalid_ladder_math_sizes": planning.invalid_ladder_math_sizes,
            }),
            Some(&market.market_id),
        )?;
    }
    let plans = planning.plans;
    if plans.is_empty() {
        LogContext::MARKET_CYCLE.dual_audit(
            store,
            Level::DEBUG,
            "coin ops no plans",
            COIN_OPS_NO_PLANS,
            &json!({"market_id": market.market_id}),
            Some(&market.market_id),
        )?;
        return Ok(None);
    }

    let projected_fee = projected_coin_ops_fee_mojos(
        &plans,
        program.coin_ops_split_fee_mojos,
        program.coin_ops_combine_fee_mojos,
    );
    let spent_today = store.get_daily_fee_spent_mojos_utc()?;
    let (executable_plans, overflow_plans) = partition_plans_by_budget(
        &plans,
        program.coin_ops_split_fee_mojos,
        program.coin_ops_combine_fee_mojos,
        spent_today,
        program.coin_ops_max_daily_fee_budget_mojos,
    );
    Ok(Some(CoinOpsPlanningResult {
        plans,
        projected_fee,
        spent_today,
        executable_plans,
        overflow_plans,
    }))
}

async fn execute_coin_ops_plans(
    store: &SqliteStore,
    ctx: &MarketCycleContext<'_>,
    market: &MarketConfig,
    program: &ManagerProgramConfig,
    planning: &CoinOpsPlanningResult,
) -> SignerResult<CoinOpExecutionResult> {
    let operator_network = ctx.resources.network.as_str();
    // Durable watches only (healed from Dexie at reconcile when missing).
    let watched_coin_ids = store.list_watched_coin_ids_for_market(&market.market_id)?;
    if planning.executable_plans.is_empty() {
        return Ok(CoinOpExecutionResult {
            dry_run: program.runtime_dry_run,
            planned_count: 0,
            executed_count: 0,
            status: "skipped_fee_budget".to_string(),
            items: Vec::new(),
            signer_selection: json!({
                "selected_source": "signer_registry",
                "key_id": market.signer_key_id,
                "network": operator_network,
            }),
        });
    }

    match ctx.gated_market(market) {
        Ok(gated) => {
            Ok(
                execute_managed_coin_op_plans(gated, &planning.executable_plans, &watched_coin_ids)
                    .await,
            )
        }
        Err(err) => Ok(skipped_coin_ops_result(
            program,
            market,
            ctx.resources.network.as_str(),
            &planning.executable_plans,
            &signer_execution_skip_reason(&err),
        )),
    }
}

fn apply_overflow_plan_skips(execution: &mut CoinOpExecutionResult, overflow_plans: &[CoinOpPlan]) {
    execution
        .items
        .extend(overflow_plans.iter().map(|plan| CoinOpExecItem {
            op_type: plan.op_type.as_str().to_string(),
            size_base_units: plan.size_base_units,
            op_count: plan.op_count,
            status: "skipped".to_string(),
            reason: "fee_budget_guard".to_string(),
            operation_id: None,
        }));
}

fn record_coin_ops_phase_audit(
    store: &SqliteStore,
    market: &MarketConfig,
    program: &ManagerProgramConfig,
    planning: &CoinOpsPlanningResult,
    execution: &CoinOpExecutionResult,
) -> SignerResult<()> {
    if !planning.overflow_plans.is_empty() {
        LogContext::MARKET_CYCLE.dual_audit(
            store,
            Level::WARN,
            "coin ops partial or skipped fee budget",
            COIN_OPS_PARTIAL_OR_SKIPPED_FEE_BUDGET,
            &json!({
                "market_id": market.market_id,
                "spent_today_mojos": planning.spent_today,
                "projected_mojos": planning.projected_fee,
                "max_daily_fee_budget_mojos": program.coin_ops_max_daily_fee_budget_mojos,
                "overflow_plans": planning.overflow_plans
                    .iter()
                    .map(plan_summary)
                    .collect::<Vec<_>>(),
            }),
            Some(&market.market_id),
        )?;
    }

    LogContext::MARKET_CYCLE.dual_audit(
        store,
        Level::INFO,
        "coin ops plan",
        COIN_OPS_PLAN,
        &json!({
            "market_id": market.market_id,
            "projected_fee_mojos": planning.projected_fee,
            "spent_today_mojos": planning.spent_today,
            "plans": planning.plans.iter().map(plan_summary).collect::<Vec<_>>(),
            "execution": execution_payload(execution),
        }),
        Some(&market.market_id),
    )?;

    if planning.executable_plans.is_empty() {
        LogContext::MARKET_CYCLE.dual_audit(
            store,
            Level::WARN,
            "coin ops skipped fee budget",
            COIN_OPS_SKIPPED_FEE_BUDGET,
            &json!({
                "market_id": market.market_id,
                "plan_count": planning.plans.len(),
                "overflow_count": planning.overflow_plans.len(),
            }),
            Some(&market.market_id),
        )?;
    } else {
        LogContext::MARKET_CYCLE.dual_audit(
            store,
            Level::INFO,
            "coin ops executed",
            COIN_OPS_EXECUTED,
            &json!({
                "market_id": market.market_id,
                "plan_count": planning.plans.len(),
                "executable_count": planning.executable_plans.len(),
                "overflow_count": planning.overflow_plans.len(),
            }),
            Some(&market.market_id),
        )?;
    }
    Ok(())
}

pub async fn run_coin_ops_phase(
    store: &SqliteStore,
    ctx: &MarketCycleContext<'_>,
    market: &MarketConfig,
    wallet_bucket_counts: &BTreeMap<i64, i64>,
    active_counts: &BTreeMap<i64, i64>,
    newly_executed_counts: &BTreeMap<i64, i64>,
) -> SignerResult<()> {
    let program = ctx.resources.program();
    let sell_ladder = market.ladders.get("sell").cloned().unwrap_or_default();
    if sell_ladder.is_empty() {
        LogContext::MARKET_CYCLE.dual_audit(
            store,
            Level::DEBUG,
            "coin ops no plans",
            COIN_OPS_NO_PLANS,
            &json!({"market_id": market.market_id, "reason": "empty_sell_ladder"}),
            Some(&market.market_id),
        )?;
        return Ok(());
    }

    let valid_ladder = build_valid_sell_ladder(store, market, &sell_ladder)?;
    if valid_ladder.is_empty() {
        return Ok(());
    }

    let Some(planning) = plan_coin_ops_for_market(
        store,
        program,
        market,
        &valid_ladder,
        wallet_bucket_counts,
        active_counts,
        newly_executed_counts,
    )?
    else {
        return Ok(());
    };

    let mut execution = execute_coin_ops_plans(store, ctx, market, program, &planning).await?;
    apply_overflow_plan_skips(&mut execution, &planning.overflow_plans);
    execution.planned_count = planning.plans.len();

    record_coin_ops_phase_audit(store, market, program, &planning, &execution)?;
    persist_coin_op_execution(store, market, program, &execution)?;
    Ok(())
}

fn skipped_coin_ops_result(
    program: &ManagerProgramConfig,
    market: &MarketConfig,
    operator_network: &str,
    plans: &[CoinOpPlan],
    reason: &str,
) -> CoinOpExecutionResult {
    CoinOpExecutionResult {
        dry_run: program.runtime_dry_run,
        planned_count: plans.len(),
        executed_count: 0,
        status: "skipped".to_string(),
        items: plans
            .iter()
            .map(|plan| CoinOpExecItem {
                op_type: plan.op_type.as_str().to_string(),
                size_base_units: plan.size_base_units,
                op_count: plan.op_count,
                status: "skipped".to_string(),
                reason: reason.to_string(),
                operation_id: None,
            })
            .collect(),
        signer_selection: json!({
            "selected_source": "signer_registry",
            "key_id": market.signer_key_id,
            "network": operator_network,
        }),
    }
}

fn plan_summary(plan: &CoinOpPlan) -> serde_json::Value {
    json!({
        "op_type": plan.op_type.as_str(),
        "size_base_units": plan.size_base_units,
        "op_count": plan.op_count,
        "reason": plan.reason.as_str(),
    })
}

fn execution_payload(execution: &CoinOpExecutionResult) -> serde_json::Value {
    json!({
        "dry_run": execution.dry_run,
        "planned_count": execution.planned_count,
        "executed_count": execution.executed_count,
        "status": execution.status,
        "signer_selection": execution.signer_selection,
        "items": execution
            .items
            .iter()
            .map(|item| json!({
                "op_type": item.op_type,
                "size_base_units": item.size_base_units,
                "op_count": item.op_count,
                "status": item.status,
                "reason": item.reason,
                "operation_id": item.operation_id,
            }))
            .collect::<Vec<_>>(),
    })
}
