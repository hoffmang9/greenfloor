use crate::async_boundary::ManagerCommandFuture;
use serde_json::json;

use crate::coin_ops::evaluate_coin_split_gate;
#[cfg(test)]
use crate::coin_ops::execution::CoinOpTestOverrides;
use crate::coin_ops::{coin_op_non_negative_u64, i64_to_usize};
use crate::error::{SignerError, SignerResult};
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::ladder::{resolve_split_targets, split_required_count};

use super::loop_common::finish_coin_op_command;
use super::loop_context::{prepare_coin_op_loop_common, CoinOpLoopPrep};
use super::split_iteration::{run_split_iteration, SplitIterationParams};
use super::until_ready::{run_until_ready_loop, UntilReadyLoopConfig, UntilReadyWaitMode};

#[derive(Debug, Clone)]
pub struct CoinSplitGating {
    pub allow_lock_all_spendable: bool,
    pub force_split_when_ready: bool,
}

#[derive(Debug, Clone)]
pub struct CoinSplitBehavior {
    pub wait: UntilReadyWaitMode,
    pub gating: CoinSplitGating,
}

impl CoinSplitBehavior {
    #[must_use]
    #[allow(clippy::fn_params_excessive_bools)]
    pub fn from_cli(
        until_ready: bool,
        no_wait: bool,
        allow_lock_all_spendable: bool,
        force_split_when_ready: bool,
    ) -> Self {
        Self {
            wait: UntilReadyWaitMode::from_cli_flags(until_ready, no_wait),
            gating: CoinSplitGating {
                allow_lock_all_spendable,
                force_split_when_ready,
            },
        }
    }
}

pub struct CoinSplitRequest<'a> {
    pub mgr: &'a ManagerContext,
    pub network: &'a str,
    pub market_id: Option<&'a str>,
    pub pair: Option<&'a str>,
    pub coin_ids: &'a [String],
    pub amount_per_coin: i64,
    pub number_of_coins: i64,
    pub behavior: CoinSplitBehavior,
    pub size_base_units: Option<i64>,
    pub max_iterations: i32,
}

struct SplitLoopContext<'a> {
    common: super::loop_context::CoinOpLoopCommon<'a>,
    amount_per_coin: i64,
    number_of_coins: i64,
    required_amount: i64,
    output_amounts: Vec<u64>,
    split_fee: u64,
    gating: CoinSplitGating,
}

async fn prepare_split_loop_context(
    request: CoinSplitRequest<'_>,
) -> SignerResult<SplitLoopContext<'_>> {
    let CoinSplitRequest {
        mgr,
        network,
        market_id,
        pair,
        coin_ids,
        amount_per_coin,
        number_of_coins,
        behavior,
        size_base_units,
        max_iterations: _,
    } = request;
    let CoinSplitBehavior { wait, gating } = behavior;
    let common = prepare_coin_op_loop_common(CoinOpLoopPrep {
        mgr,
        network,
        market_id,
        pair,
        asset_id: None,
        wait,
        size_base_units,
        coin_ids,
    })
    .await?;
    let (amount_per_coin, number_of_coins) = resolve_split_targets(
        &common.exec_ctx.market,
        amount_per_coin,
        number_of_coins,
        size_base_units,
    )?;
    if amount_per_coin <= 0 || number_of_coins <= 0 {
        return Err(SignerError::Other(
            "amount_per_coin and number_of_coins must be positive".to_string(),
        ));
    }
    let amount_per_coin_mojos =
        amount_per_coin.saturating_mul(common.exec_ctx.base_unit_mojo_multiplier);
    let required_amount = amount_per_coin_mojos.saturating_mul(number_of_coins);
    let output_count = i64_to_usize(number_of_coins, "split.number_of_coins")?;
    let amount_u64 =
        coin_op_non_negative_u64(amount_per_coin_mojos, "split.amount_per_coin_mojos")?;
    let split_fee = coin_op_non_negative_u64(
        common.exec_ctx.program.coin_ops_split_fee_mojos,
        "program.coin_ops_split_fee_mojos",
    )?;
    Ok(SplitLoopContext {
        common,
        amount_per_coin,
        number_of_coins,
        required_amount,
        output_amounts: vec![amount_u64; output_count],
        split_fee,
        gating,
    })
}

async fn run_coin_split_from_context(
    mgr: &ManagerContext,
    max_iterations: i32,
    SplitLoopContext {
        common,
        amount_per_coin,
        number_of_coins,
        required_amount,
        output_amounts,
        split_fee,
        gating,
    }: SplitLoopContext<'_>,
) -> SignerResult<i32> {
    let super::loop_context::CoinOpLoopCommon {
        exec_ctx,
        wait,
        explicit_coin_ids,
        resolved_asset_id,
        ladder_entry: split_target,
        coin_ids,
    } = common;
    let CoinSplitGating {
        allow_lock_all_spendable,
        force_split_when_ready,
    } = gating;

    let (operations, completion) = Box::pin(run_until_ready_loop(
        &exec_ctx,
        UntilReadyLoopConfig {
            wait,
            max_iterations,
            explicit_coin_ids,
            stop_when_gate_ready: !force_split_when_ready,
        },
        |gate_coins| {
            split_target.as_ref().map(|entry| {
                evaluate_coin_split_gate(
                    gate_coins,
                    &resolved_asset_id,
                    amount_per_coin.saturating_mul(exec_ctx.base_unit_mojo_multiplier),
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
                no_wait: wait.no_wait,
                allow_lock_all_spendable,
            })
        },
    ))
    .await?;

    finish_coin_op_command(
        mgr,
        wait,
        completion,
        json!({
            "op": "coin-split",
            "coin_selection_mode": if explicit_coin_ids { "explicit" } else { "adapter_auto_select" },
            "amount_per_coin": amount_per_coin,
            "number_of_coins": number_of_coins,
            "resolved_asset_id": exec_ctx.resolved_base_asset_id,
            "until_ready": wait.until_ready,
            "max_iterations": max_iterations.max(1),
            "operations": operations,
        }),
    )
}

pub fn run_coin_split(request: CoinSplitRequest<'_>) -> ManagerCommandFuture<'_> {
    Box::pin(run_coin_split_async(request))
}

async fn run_coin_split_async(request: CoinSplitRequest<'_>) -> SignerResult<i32> {
    let mgr = request.mgr;
    let max_iterations = request.max_iterations;
    let ctx = prepare_split_loop_context(request).await?;
    run_coin_split_from_context(mgr, max_iterations, ctx).await
}

#[cfg(test)]
pub fn run_coin_split_with_test_overrides(
    request: CoinSplitRequest<'_>,
    test_overrides: CoinOpTestOverrides,
) -> ManagerCommandFuture<'_> {
    Box::pin(run_coin_split_with_test_overrides_async(
        request,
        test_overrides,
    ))
}

#[cfg(test)]
async fn run_coin_split_with_test_overrides_async(
    request: CoinSplitRequest<'_>,
    test_overrides: CoinOpTestOverrides,
) -> SignerResult<i32> {
    let mgr = request.mgr;
    let max_iterations = request.max_iterations;
    let mut ctx = prepare_split_loop_context(request).await?;
    ctx.common.exec_ctx.test_overrides = test_overrides;
    run_coin_split_from_context(mgr, max_iterations, ctx).await
}
