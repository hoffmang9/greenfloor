//! Coin-op until-ready iteration shell shared by split and combine CLI loops.

use std::future::Future;

use serde::Serialize;
use serde_json::Value;

use crate::coin_ops::execution::CoinOpExecContext;
use crate::coin_ops::{coin_op_should_stop, SpendableCoin};
use crate::error::SignerResult;

use super::context::spendable_coins_for_gate;

const ITERATION_SLEEP_SECS: u64 = 2;

#[derive(Debug, Clone, Copy)]
pub struct UntilReadyWaitMode {
    pub until_ready: bool,
    pub no_wait: bool,
}

impl UntilReadyWaitMode {
    #[must_use]
    pub fn from_cli_flags(until_ready: bool, no_wait: bool) -> Self {
        Self {
            until_ready,
            no_wait,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UntilReadyLoopConfig {
    pub wait: UntilReadyWaitMode,
    pub max_iterations: i32,
    pub explicit_coin_ids: bool,
    /// When true, stop before the iteration body when the gate reports ready.
    pub stop_when_gate_ready: bool,
}

pub enum LoopIterationOutcome {
    Continue { operation: Value },
    Exit { code: i32, payload: Option<Value> },
}

#[derive(Debug, Clone)]
pub enum UntilReadyCompletion {
    Completed { stop_reason: String },
    Exit { code: i32, payload: Option<Value> },
}

fn gate_ready_for_stop<G>(
    config: &UntilReadyLoopConfig,
    gate: &G,
    gate_ready: &impl Fn(&G) -> bool,
) -> bool {
    let ready = gate_ready(gate);
    if ready && !config.stop_when_gate_ready {
        false
    } else {
        ready
    }
}

fn iteration_stop_reason(
    config: &UntilReadyLoopConfig,
    gate_ready_value: Option<bool>,
    iteration: i32,
    max_iterations: i32,
    after_iteration_body: bool,
) -> Option<&'static str> {
    if !after_iteration_body
        && config.wait.until_ready
        && config.stop_when_gate_ready
        && gate_ready_value == Some(true)
    {
        return Some("ready");
    }
    let (should_stop, reason) = coin_op_should_stop(
        config.wait.until_ready,
        gate_ready_value,
        config.explicit_coin_ids,
        i64::from(iteration),
        i64::from(max_iterations),
    );
    if !should_stop {
        return None;
    }
    if after_iteration_body || config.wait.until_ready {
        Some(reason)
    } else {
        None
    }
}

pub async fn run_until_ready_loop<G, GateReady, RunIteration, Fut>(
    ctx: &CoinOpExecContext,
    config: UntilReadyLoopConfig,
    mut evaluate_gate: impl FnMut(&[Value]) -> Option<G>,
    gate_ready: GateReady,
    mut run_iteration: RunIteration,
) -> SignerResult<(Vec<Value>, UntilReadyCompletion)>
where
    G: Serialize,
    GateReady: Fn(&G) -> bool,
    RunIteration: FnMut(i32, Vec<SpendableCoin>, Option<Value>) -> Fut,
    Fut: Future<Output = SignerResult<LoopIterationOutcome>>,
{
    let max_iterations = config.max_iterations.max(1);
    let mut operations = Vec::new();
    let mut stop_reason = "single_pass".to_string();

    for iteration in 1..=max_iterations {
        let spendable = ctx.list_spendable_coins().await?;
        let gate_coins = spendable_coins_for_gate(&spendable);
        let gate = evaluate_gate(&gate_coins);
        let gate_json = gate
            .as_ref()
            .and_then(|gate| serde_json::to_value(gate).ok());
        let gate_ready_value = gate
            .as_ref()
            .map(|gate| gate_ready_for_stop(&config, gate, &gate_ready));

        if let Some(reason) =
            iteration_stop_reason(&config, gate_ready_value, iteration, max_iterations, false)
        {
            stop_reason = reason.to_string();
            break;
        }

        match run_iteration(iteration, spendable, gate_json).await? {
            LoopIterationOutcome::Continue { operation } => {
                operations.push(operation);
            }
            LoopIterationOutcome::Exit { code, payload } => {
                return Ok((operations, UntilReadyCompletion::Exit { code, payload }));
            }
        }

        if let Some(reason) =
            iteration_stop_reason(&config, gate_ready_value, iteration, max_iterations, true)
        {
            stop_reason = reason.to_string();
            break;
        }
        if config.wait.no_wait {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(ITERATION_SLEEP_SECS)).await;
    }

    Ok((operations, UntilReadyCompletion::Completed { stop_reason }))
}

pub fn until_ready_exit_code(until_ready: bool, stop_reason: &str) -> i32 {
    if until_ready && stop_reason != "ready" {
        2
    } else {
        0
    }
}
