use std::path::Path;

use serde_json::json;

use crate::coin_ops::evaluate_coin_combine_gate;
use crate::error::{SignerError, SignerResult};

use super::combine_iteration::run_combine_iteration;
use super::context::{build_coin_op_exec_context, combine_gate_to_json};
use super::loop_common::validate_until_ready_mode;
use super::until_ready::{run_until_ready_loop, until_ready_exit_code, UntilReadyLoopConfig};
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
        .transpose()?
        .cloned();
    let explicit_coin_ids = !coin_ids.is_empty();
    let resolved_asset_id = ctx.resolved_base_asset_id.clone();
    let combine_fee = ctx.program.coin_ops_combine_fee_mojos.max(0) as u64;

    let (operations, completion) = run_until_ready_loop(
        &ctx,
        UntilReadyLoopConfig {
            until_ready,
            no_wait,
            max_iterations,
            explicit_coin_ids,
            stop_when_gate_ready: true,
        },
        |gate_coins| {
            combine_target.as_ref().map(|entry| {
                evaluate_coin_combine_gate(
                    gate_coins,
                    &resolved_asset_id,
                    target_coin_amount_mojos,
                    combine_threshold_count(entry),
                )
            })
        },
        |gate| gate.ready,
        combine_gate_to_json,
        |iteration, spendable, gate_json| {
            run_combine_iteration(
                &ctx,
                iteration,
                spendable,
                gate_json,
                number_of_coins,
                target_coin_amount_mojos,
                coin_ids,
                combine_fee,
                no_wait,
            )
        },
    )
    .await?;

    if let Some(code) = completion.exit_code() {
        return Ok(code);
    }

    emit_json(&json!({
        "op": "coin-combine",
        "coin_selection_mode": if explicit_coin_ids { "explicit" } else { "adapter_auto_select" },
        "number_of_coins": number_of_coins,
        "resolved_asset_id": ctx.resolved_base_asset_id,
        "until_ready": until_ready,
        "max_iterations": max_iterations.max(1),
        "stop_reason": completion.stop_reason().unwrap_or("single_pass"),
        "operations": operations,
    }))?;
    Ok(until_ready_exit_code(
        until_ready,
        completion.stop_reason().unwrap_or("single_pass"),
    ))
}
