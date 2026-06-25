use crate::async_boundary::ManagerCommandFuture;
use serde_json::json;

use crate::coin_ops::evaluate_coin_combine_gate;
use crate::error::{SignerError, SignerResult};
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::ladder::resolve_combine_count;
use crate::offer::pricing::combine_threshold_count;

use super::combine_iteration::{run_combine_iteration, CombineIterationParams};
use super::loop_common::finish_coin_op_command;
use super::loop_context::{prepare_coin_op_loop_common, CoinOpLoopPrep};
use super::until_ready::{run_until_ready_loop, UntilReadyLoopConfig, UntilReadyWaitMode};

/// Combine-specific CLI behavior. Unlike split, combine has no spend gating flags —
/// readiness is driven only by inventory gate evaluation inside the until-ready loop.
#[derive(Debug, Clone)]
pub struct CoinCombineBehavior {
    pub wait: UntilReadyWaitMode,
}

impl CoinCombineBehavior {
    #[must_use]
    pub fn from_cli(until_ready: bool, no_wait: bool) -> Self {
        Self {
            wait: UntilReadyWaitMode::from_cli_flags(until_ready, no_wait),
        }
    }
}

pub struct CoinCombineRequest<'a> {
    pub mgr: &'a ManagerContext,
    pub network: &'a str,
    pub market_id: Option<&'a str>,
    pub pair: Option<&'a str>,
    pub coin_ids: &'a [String],
    pub number_of_coins: i64,
    pub asset_id: Option<&'a str>,
    pub behavior: CoinCombineBehavior,
    pub size_base_units: Option<i64>,
    pub max_iterations: i32,
}

struct CombineLoopContext<'a> {
    common: super::loop_context::CoinOpLoopCommon<'a>,
    behavior: CoinCombineBehavior,
    number_of_coins: i64,
    target_coin_amount_mojos: i64,
}

async fn prepare_combine_loop_context(
    request: CoinCombineRequest<'_>,
) -> SignerResult<CombineLoopContext<'_>> {
    let CoinCombineRequest {
        mgr,
        network,
        market_id,
        pair,
        coin_ids,
        number_of_coins,
        asset_id,
        behavior,
        size_base_units,
        max_iterations: _,
    } = request;
    let CoinCombineBehavior { wait } = behavior;
    let common = prepare_coin_op_loop_common(CoinOpLoopPrep {
        mgr,
        network,
        market_id,
        pair,
        asset_id,
        wait,
        size_base_units,
        coin_ids,
    })
    .await?;
    let number_of_coins = resolve_combine_count(
        &common.exec_ctx.gated.market_row,
        number_of_coins,
        size_base_units,
    )?;
    if number_of_coins <= 1 {
        return Err(SignerError::Other(
            "number_of_coins must be > 1".to_string(),
        ));
    }
    let target_coin_amount_mojos = size_base_units
        .unwrap_or(0)
        .max(0)
        .saturating_mul(common.exec_ctx.base_unit_mojo_multiplier);
    Ok(CombineLoopContext {
        common,
        behavior,
        number_of_coins,
        target_coin_amount_mojos,
    })
}

pub fn run_coin_combine(request: CoinCombineRequest<'_>) -> ManagerCommandFuture<'_> {
    Box::pin(run_coin_combine_async(request))
}

#[allow(clippy::large_futures)]
async fn run_coin_combine_async(request: CoinCombineRequest<'_>) -> SignerResult<i32> {
    let mgr = request.mgr;
    let max_iterations = request.max_iterations;
    let CombineLoopContext {
        common,
        behavior,
        number_of_coins,
        target_coin_amount_mojos,
    } = prepare_combine_loop_context(request).await?;
    let CoinCombineBehavior { wait } = behavior;
    let super::loop_context::CoinOpLoopCommon {
        exec_ctx,
        explicit_coin_ids,
        resolved_asset_id,
        ladder_entry: combine_target,
        coin_ids,
        ..
    } = common;
    let combine_threshold = combine_target
        .as_ref()
        .map(|entry| combine_threshold_count(entry.target_count, entry.combine_when_excess_factor))
        .transpose()?;

    let (operations, completion) = run_until_ready_loop(
        &exec_ctx,
        UntilReadyLoopConfig {
            wait,
            max_iterations,
            explicit_coin_ids,
            stop_when_gate_ready: true,
        },
        |gate_coins| {
            combine_threshold.map(|threshold| {
                evaluate_coin_combine_gate(
                    gate_coins,
                    &resolved_asset_id,
                    target_coin_amount_mojos,
                    threshold,
                )
            })
        },
        |gate| gate.ready,
        |iteration, spendable, gate_json| {
            run_combine_iteration(CombineIterationParams {
                ctx: &exec_ctx,
                iteration,
                spendable,
                gate_json,
                number_of_coins,
                target_coin_amount_mojos,
                coin_ids,
                no_wait: wait.no_wait,
            })
        },
    )
    .await?;

    finish_coin_op_command(
        mgr,
        wait,
        completion,
        json!({
            "op": "coin-combine",
            "coin_selection_mode": if explicit_coin_ids { "explicit" } else { "adapter_auto_select" },
            "number_of_coins": number_of_coins,
            "resolved_asset_id": exec_ctx.resolved_base_asset_id,
            "until_ready": wait.until_ready,
            "max_iterations": max_iterations.max(1),
            "operations": operations,
        }),
    )
}
