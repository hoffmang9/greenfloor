use serde_json::json;

use crate::coin_ops::evaluate_coin_split_gate;
use crate::coin_ops::{coin_op_non_negative_u64, i64_to_usize};
use crate::error::{SignerError, SignerResult};
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::ladder::{
    resolve_split_targets, sell_ladder_entry_for_size, split_required_count,
};

use super::context::build_coin_op_exec_context;
use super::loop_common::{finish_coin_op_command, validate_until_ready_mode};
use super::split_iteration::{run_split_iteration, SplitIterationParams};
use super::until_ready::{run_until_ready_loop, UntilReadyLoopConfig};

pub struct CoinSplitRequest<'a> {
    pub mgr: &'a ManagerContext,
    pub network: &'a str,
    pub market_id: Option<&'a str>,
    pub pair: Option<&'a str>,
    pub coin_ids: &'a [String],
    pub amount_per_coin: i64,
    pub number_of_coins: i64,
    pub no_wait: bool,
    pub size_base_units: Option<i64>,
    pub until_ready: bool,
    pub max_iterations: i32,
    pub allow_lock_all_spendable: bool,
    pub force_split_when_ready: bool,
}

pub async fn run_coin_split(request: CoinSplitRequest<'_>) -> SignerResult<i32> {
    let CoinSplitRequest {
        mgr,
        network,
        market_id,
        pair,
        coin_ids,
        amount_per_coin,
        number_of_coins,
        no_wait,
        size_base_units,
        until_ready,
        max_iterations,
        allow_lock_all_spendable,
        force_split_when_ready,
    } = request;
    validate_until_ready_mode(until_ready, no_wait, size_base_units)?;
    let exec_ctx = build_coin_op_exec_context(
        &mgr.program_config,
        &mgr.markets_config,
        mgr.testnet_markets_path(),
        network,
        market_id,
        pair,
        None,
    )
    .await?;
    let (amount_per_coin, number_of_coins) = resolve_split_targets(
        &exec_ctx.market,
        amount_per_coin,
        number_of_coins,
        size_base_units,
    )?;
    if amount_per_coin <= 0 || number_of_coins <= 0 {
        return Err(SignerError::Other(
            "amount_per_coin and number_of_coins must be positive".to_string(),
        ));
    }
    let amount_per_coin_mojos = amount_per_coin.saturating_mul(exec_ctx.base_unit_mojo_multiplier);
    let required_amount = amount_per_coin_mojos.saturating_mul(number_of_coins);
    let split_target = size_base_units
        .filter(|value| *value > 0)
        .map(|size| sell_ladder_entry_for_size(&exec_ctx.market, size))
        .transpose()?
        .cloned();
    let explicit_coin_ids = !coin_ids.is_empty();
    let resolved_asset_id = exec_ctx.resolved_base_asset_id.clone();
    let output_count = i64_to_usize(number_of_coins, "split.number_of_coins")?;
    let amount_u64 =
        coin_op_non_negative_u64(amount_per_coin_mojos, "split.amount_per_coin_mojos")?;
    let output_amounts: Vec<u64> = vec![amount_u64; output_count];
    let split_fee = coin_op_non_negative_u64(
        exec_ctx.program.coin_ops_split_fee_mojos,
        "program.coin_ops_split_fee_mojos",
    )?;

    let (operations, completion) = run_until_ready_loop(
        &exec_ctx,
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
            run_split_iteration(SplitIterationParams {
                ctx: &exec_ctx,
                iteration,
                spendable,
                gate_json,
                explicit_coin_ids,
                coin_ids,
                required_amount,
                output_amounts: &output_amounts,
                split_fee,
                no_wait,
                allow_lock_all_spendable,
            })
        },
    )
    .await?;

    finish_coin_op_command(
        mgr,
        until_ready,
        completion,
        json!({
            "op": "coin-split",
            "coin_selection_mode": if explicit_coin_ids { "explicit" } else { "adapter_auto_select" },
            "amount_per_coin": amount_per_coin,
            "number_of_coins": number_of_coins,
            "resolved_asset_id": exec_ctx.resolved_base_asset_id,
            "until_ready": until_ready,
            "max_iterations": max_iterations.max(1),
            "operations": operations,
        }),
    )
}
