use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Value};

use crate::coin_ops::{
    coin_op_target_amount_allowed, effective_sell_bucket_counts_for_coin_ops,
    partition_plans_by_budget, plan_coin_ops, projected_coin_ops_fee_mojos, BucketSpec, CoinOpPlan,
};
use crate::config::{ManagerProgramConfig, MarketConfig};
use crate::error::SignerResult;
use crate::hex::default_mojo_multiplier_for_asset;
use crate::storage::SqliteStore;

use super::coin_ops_execution::{
    execute_managed_coin_op_plans, persist_coin_op_execution, watched_coin_ids_for_market,
    CoinOpExecItem, CoinOpExecutionResult,
};

pub async fn run_coin_ops_phase(
    store: &SqliteStore,
    market: &MarketConfig,
    program: &ManagerProgramConfig,
    program_path: &Path,
    offers: &[Value],
    wallet_bucket_counts: &BTreeMap<i64, i64>,
    active_counts: &BTreeMap<i64, i64>,
    newly_executed_counts: &BTreeMap<i64, i64>,
) -> SignerResult<()> {
    let sell_ladder = market.ladders.get("sell").cloned().unwrap_or_default();
    if sell_ladder.is_empty() {
        store.add_audit_event(
            "coin_ops_no_plans",
            &json!({"market_id": market.market_id, "reason": "empty_sell_ladder"}),
            Some(&market.market_id),
        )?;
        return Ok(());
    }

    let bucket_counts = effective_sell_bucket_counts_for_coin_ops(
        &sell_ladder
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
    let base_unit_multiplier = default_mojo_multiplier_for_asset(market.base_asset.trim()) as i64;
    let mut valid_ladder = Vec::new();
    let mut invalid_buckets = Vec::new();
    for entry in &sell_ladder {
        if entry.size_base_units <= 0 {
            continue;
        }
        let target_amount_mojos = entry.size_base_units.saturating_mul(base_unit_multiplier);
        if coin_op_target_amount_allowed(target_amount_mojos, market.base_asset.trim()) {
            valid_ladder.push(entry.clone());
            continue;
        }
        invalid_buckets.push(json!({
            "size_base_units": entry.size_base_units,
            "target_amount_mojos": target_amount_mojos,
        }));
    }
    if !invalid_buckets.is_empty() {
        store.add_audit_event(
            "coin_ops_skip_sub_minimum_target_amount",
            &json!({
                "market_id": market.market_id,
                "invalid_bucket_count": invalid_buckets.len(),
                "invalid_buckets": invalid_buckets,
            }),
            Some(&market.market_id),
        )?;
    }
    if valid_ladder.is_empty() {
        return Ok(());
    }

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
    let plans = plan_coin_ops(
        &buckets,
        program.coin_ops_max_operations_per_run,
        program.coin_ops_max_daily_fee_budget_mojos,
        program.coin_ops_split_fee_mojos,
        program.coin_ops_combine_fee_mojos,
    );
    if plans.is_empty() {
        store.add_audit_event(
            "coin_ops_no_plans",
            &json!({"market_id": market.market_id}),
            Some(&market.market_id),
        )?;
        return Ok(());
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

    let watched_coin_ids = watched_coin_ids_for_market(store, &market.market_id, offers)?;
    let mut execution = if executable_plans.is_empty() {
        CoinOpExecutionResult {
            dry_run: program.runtime_dry_run,
            planned_count: 0,
            executed_count: 0,
            status: "skipped_fee_budget".to_string(),
            items: Vec::new(),
            signer_selection: json!({
                "selected_source": "signer_registry",
                "key_id": market.signer_key_id,
                "network": program.network,
            }),
        }
    } else {
        execute_managed_coin_op_plans(
            program_path,
            market,
            program,
            &executable_plans,
            &watched_coin_ids,
        )
        .await
    };

    if !overflow_plans.is_empty() {
        store.add_audit_event(
            "coin_ops_partial_or_skipped_fee_budget",
            &json!({
                "market_id": market.market_id,
                "spent_today_mojos": spent_today,
                "projected_mojos": projected_fee,
                "max_daily_fee_budget_mojos": program.coin_ops_max_daily_fee_budget_mojos,
                "overflow_plans": overflow_plans
                    .iter()
                    .map(plan_summary)
                    .collect::<Vec<_>>(),
            }),
            Some(&market.market_id),
        )?;
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
    execution.planned_count = plans.len();

    store.add_audit_event(
        "coin_ops_plan",
        &json!({
            "market_id": market.market_id,
            "projected_fee_mojos": projected_fee,
            "spent_today_mojos": spent_today,
            "plans": plans.iter().map(plan_summary).collect::<Vec<_>>(),
            "execution": execution_payload(&execution),
        }),
        Some(&market.market_id),
    )?;

    if !executable_plans.is_empty() {
        store.add_audit_event(
            "coin_ops_executed",
            &json!({
                "market_id": market.market_id,
                "plan_count": plans.len(),
                "executable_count": executable_plans.len(),
                "overflow_count": overflow_plans.len(),
            }),
            Some(&market.market_id),
        )?;
    } else {
        store.add_audit_event(
            "coin_ops_skipped_fee_budget",
            &json!({
                "market_id": market.market_id,
                "plan_count": plans.len(),
                "overflow_count": overflow_plans.len(),
            }),
            Some(&market.market_id),
        )?;
    }

    persist_coin_op_execution(store, market, program, &execution)?;
    Ok(())
}

fn plan_summary(plan: &CoinOpPlan) -> serde_json::Value {
    json!({
        "op_type": plan.op_type.as_str(),
        "size_base_units": plan.size_base_units,
        "op_count": plan.op_count,
        "reason": plan.reason,
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
