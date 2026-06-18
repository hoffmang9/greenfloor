use std::path::Path;

use serde_json::{json, Value};

use crate::coin_ops::{
    coin_op_should_stop, combine_output_amounts, evaluate_coin_combine_gate,
    plan_auto_combine_inputs, total_for_coin_ids, CombineInputSelectionMode,
};
use crate::error::{SignerError, SignerResult};

use super::loop_common::validate_until_ready_mode;
use super::context::{build_coin_op_exec_context, combine_gate_to_json, spendable_coins_for_gate};
use crate::manager_cli::json::emit_json;
use crate::manager_cli::ladder::{
    combine_threshold_count, resolve_combine_count, sell_ladder_entry_for_size,
};

pub async fn run_coin_combine(
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
    network: &str,
    market_id: Option<&str>,
    pair: Option<&str>,
    coin_ids: &[String],
    number_of_coins: i64,
    asset_id: Option<&str>,
    no_wait: bool,
    size_base_units: Option<i64>,
    until_ready: bool,
    max_iterations: i32,
) -> SignerResult<i32> {
    validate_until_ready_mode(until_ready, no_wait, size_base_units)?;
    let ctx = build_coin_op_exec_context(
        program_path,
        markets_path,
        testnet_markets_path,
        network,
        market_id,
        pair,
        asset_id,
    )
    .await?;
    let number_of_coins = resolve_combine_count(&ctx.market, number_of_coins, size_base_units)?;
    if number_of_coins <= 1 {
        return Err(SignerError::Other("number_of_coins must be > 1".to_string()));
    }
    let target_coin_amount_mojos = size_base_units
        .unwrap_or(0)
        .max(0)
        .saturating_mul(ctx.base_unit_mojo_multiplier);
    let combine_target = size_base_units
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
        let combine_gate = combine_target.as_ref().map(|entry| {
            evaluate_coin_combine_gate(
                &gate_coins,
                &ctx.resolved_base_asset_id,
                target_coin_amount_mojos,
                combine_threshold_count(entry),
            )
        });

        if let Some(ref gate) = combine_gate {
            if until_ready && gate.ready {
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

        let input_coin_ids = if coin_ids.is_empty() {
            plan_auto_combine_inputs(
                &spendable,
                number_of_coins as usize,
                CombineInputSelectionMode::ExactAmount,
                if target_coin_amount_mojos > 0 {
                    Some(target_coin_amount_mojos)
                } else {
                    None
                },
                None,
                Some(ctx.combine_input_cap as usize),
            )
            .map_err(|reason| SignerError::Other(reason.to_string()))?
        } else {
            coin_ids.to_vec()
        };
        if input_coin_ids.len() < 2 {
            emit_json(&json!({"error": "insufficient_combine_inputs"}))?;
            return Ok(2);
        }

        let total = total_for_coin_ids(&spendable, &input_coin_ids);
        let output_amounts = combine_output_amounts(total, 1);
        let operation_id = ctx
            .execute_mixed_split(
                output_amounts,
                &input_coin_ids,
                ctx.program.coin_ops_combine_fee_mojos.max(0) as u64,
            )
            .await?;
        operations.push(json!({
            "iteration": iteration,
            "signature_request_id": operation_id,
            "input_coin_ids": input_coin_ids,
            "waited": !no_wait,
            "denomination_readiness": combine_gate.as_ref().map(combine_gate_to_json),
        }));

        let (should_stop, reason) = coin_op_should_stop(
            until_ready,
            combine_gate.as_ref().map(|gate| gate.ready),
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
        "op": "coin-combine",
        "coin_selection_mode": if explicit_coin_ids { "explicit" } else { "adapter_auto_select" },
        "number_of_coins": number_of_coins,
        "resolved_asset_id": ctx.resolved_base_asset_id,
        "until_ready": until_ready,
        "max_iterations": max_iterations,
        "stop_reason": stop_reason,
        "operations": operations,
    }))?;
    Ok(if until_ready && stop_reason != "ready" { 2 } else { 0 })
}
