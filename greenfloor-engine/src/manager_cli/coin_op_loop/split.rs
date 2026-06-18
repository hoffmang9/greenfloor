use std::path::Path;

use serde_json::json;

use crate::coin_ops::evaluate_coin_split_gate;
use crate::error::{SignerError, SignerResult};

use super::context::build_coin_op_exec_context;
use super::loop_common::{finish_coin_op_command, validate_until_ready_mode};
use super::split_iteration::run_split_iteration;
use super::until_ready::{run_until_ready_loop, UntilReadyLoopConfig};
use crate::manager_cli::json::ManagerOutput;
use crate::manager_cli::ladder::{
    resolve_split_targets, sell_ladder_entry_for_size, split_required_count,
};

pub async fn run_coin_split(
    output: &ManagerOutput,
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
        .transpose()?
        .cloned();
    let explicit_coin_ids = !coin_ids.is_empty();
    let resolved_asset_id = ctx.resolved_base_asset_id.clone();
    let output_amounts: Vec<u64> =
        vec![amount_per_coin_mojos.max(0) as u64; number_of_coins as usize];
    let split_fee = ctx.program.coin_ops_split_fee_mojos.max(0) as u64;

    let (operations, completion) = run_until_ready_loop(
        &ctx,
        UntilReadyLoopConfig {
            until_ready,
            no_wait,
            max_iterations,
            explicit_coin_ids,
            stop_when_gate_ready: !force_split_when_ready,
        },
        |gate_coins| {
            split_target.as_ref().map(|entry| {
                evaluate_coin_split_gate(
                    gate_coins,
                    &resolved_asset_id,
                    amount_per_coin_mojos,
                    split_required_count(entry),
                )
            })
        },
        |gate| gate.ready,
        |iteration, spendable, gate_json| {
            run_split_iteration(
                &ctx,
                iteration,
                spendable,
                gate_json,
                explicit_coin_ids,
                coin_ids,
                required_amount,
                &output_amounts,
                split_fee,
                no_wait,
                allow_lock_all_spendable,
            )
        },
    )
    .await?;

    finish_coin_op_command(
        output,
        until_ready,
        completion,
        json!({
            "op": "coin-split",
            "coin_selection_mode": if explicit_coin_ids { "explicit" } else { "adapter_auto_select" },
            "amount_per_coin": amount_per_coin,
            "number_of_coins": number_of_coins,
            "resolved_asset_id": ctx.resolved_base_asset_id,
            "until_ready": until_ready,
            "max_iterations": max_iterations.max(1),
            "operations": operations,
        }),
    )
}
