use std::path::Path;

use serde_json::{json, Value};

use crate::coin_ops::{
    coin_op_should_stop, evaluate_coin_split_gate, plan_auto_split_selection,
    SplitAutoSelectPlan, SplitPlanningProfile,
};
use crate::error::{SignerError, SignerResult};

use super::loop_common::validate_until_ready_mode;
use super::context::{
    build_coin_op_exec_context, enforce_split_lockup_guardrail, gate_to_json,
    spendable_coins_for_gate, submit_split_combine_prereq, COIN_SPLIT_NO_SPENDABLE_ERROR,
};
use crate::manager_cli::json::emit_json;
use crate::manager_cli::ladder::{
    resolve_split_targets, sell_ladder_entry_for_size, split_required_count,
};

pub async fn run_coin_split(
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
    network: &str,
    market_id: Option<&str>,
    pair: Option<&str>,
    coin_ids: &[String],
    amount_per_coin: i64,
    number_of_coins: i64,
    no_wait: bool,
    size_base_units: Option<i64>,
    until_ready: bool,
    max_iterations: i32,
    allow_lock_all_spendable: bool,
    force_split_when_ready: bool,
) -> SignerResult<i32> {
    validate_until_ready_mode(until_ready, no_wait, size_base_units)?;
    let ctx = build_coin_op_exec_context(
        program_path,
        markets_path,
        testnet_markets_path,
        network,
        market_id,
        pair,
        None,
    )
    .await?;
    let (amount_per_coin, number_of_coins) =
        resolve_split_targets(&ctx.market, amount_per_coin, number_of_coins, size_base_units)?;
    if amount_per_coin <= 0 || number_of_coins <= 0 {
        return Err(SignerError::Other(
            "amount_per_coin and number_of_coins must be positive".to_string(),
        ));
    }
    let amount_per_coin_mojos =
        amount_per_coin.saturating_mul(ctx.base_unit_mojo_multiplier);
    let required_amount = amount_per_coin_mojos.saturating_mul(number_of_coins);
    let split_target = size_base_units
        .filter(|value| *value > 0)
        .map(|size| sell_ladder_entry_for_size(&ctx.market, size))
        .transpose()?;
    let max_iterations = max_iterations.max(1);
    let mut operations = Vec::new();
    let mut stop_reason = "single_pass".to_string();
    let explicit_coin_ids = !coin_ids.is_empty();

    for iteration in 1..=max_iterations {
        let spendable = ctx.list_spendable_coins().await?;
        let gate_coins = spendable_coins_for_gate(&spendable);
        let split_gate = split_target
            .as_ref()
            .map(|entry| {
                evaluate_coin_split_gate(
                    &gate_coins,
                    &ctx.resolved_base_asset_id,
                    amount_per_coin_mojos,
                    split_required_count(entry),
                )
            });

        if let Some(ref gate) = split_gate {
            if until_ready && gate.ready && !force_split_when_ready {
                stop_reason = "ready".to_string();
                break;
            }
            let (should_stop, reason) = coin_op_should_stop(
                until_ready,
                Some(gate.ready),
                explicit_coin_ids,
                i64::from(iteration),
                i64::from(max_iterations),
            );
            if should_stop && until_ready {
                stop_reason = reason.to_string();
                break;
            }
        }

        let selected_coin_ids = if explicit_coin_ids {
            coin_ids.to_vec()
        } else {
            if spendable.is_empty() {
                emit_json(&json!({"error": COIN_SPLIT_NO_SPENDABLE_ERROR}))?;
                return Ok(2);
            }
            match plan_auto_split_selection(
                &spendable,
                required_amount,
                ctx.market.base_asset.trim(),
                SplitPlanningProfile::CliAuto,
                ctx.combine_input_cap,
                Some(iteration == 1),
            ) {
                SplitAutoSelectPlan::CombinePrereq(prereq) => {
                    let operation_id = submit_split_combine_prereq(&ctx, &prereq).await?;
                    operations.push(json!({
                        "iteration": iteration,
                        "op": "combine-prereq",
                        "signature_request_id": operation_id,
                        "input_coin_ids": prereq.input_coin_ids,
                        "waited": !no_wait,
                    }));
                    if no_wait {
                        stop_reason = "combine_prereq_submitted".to_string();
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    continue;
                }
                SplitAutoSelectPlan::Skip(skip) => {
                    emit_json(&json!({"error": skip.reason}))?;
                    return Ok(2);
                }
                SplitAutoSelectPlan::Coin(plan) => vec![plan.coin_id],
            }
        };

        if let Some(code) = enforce_split_lockup_guardrail(
            &spendable,
            &selected_coin_ids,
            allow_lock_all_spendable,
            &ctx.resolved_base_asset_id,
        )? {
            return Ok(code);
        }

        let operation_id = ctx
            .execute_mixed_split(
                vec![amount_per_coin_mojos.max(0) as u64; number_of_coins as usize],
                &selected_coin_ids,
                ctx.program.coin_ops_split_fee_mojos.max(0) as u64,
            )
            .await?;
        operations.push(json!({
            "iteration": iteration,
            "signature_request_id": operation_id,
            "selected_coin_ids": selected_coin_ids,
            "waited": !no_wait,
            "denomination_readiness": split_gate.as_ref().map(gate_to_json),
        }));

        let (should_stop, reason) = coin_op_should_stop(
            until_ready,
            split_gate.as_ref().map(|gate| gate.ready),
            explicit_coin_ids,
            i64::from(iteration),
            i64::from(max_iterations),
        );
        if should_stop {
            stop_reason = reason.to_string();
            break;
        }
        if no_wait {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    emit_json(&json!({
        "op": "coin-split",
        "coin_selection_mode": if explicit_coin_ids { "explicit" } else { "adapter_auto_select" },
        "amount_per_coin": amount_per_coin,
        "number_of_coins": number_of_coins,
        "resolved_asset_id": ctx.resolved_base_asset_id,
        "until_ready": until_ready,
        "max_iterations": max_iterations,
        "stop_reason": stop_reason,
        "operations": operations,
    }))?;
    Ok(if until_ready && stop_reason != "ready" { 2 } else { 0 })
}
