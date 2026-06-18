//! Coin-op until-ready iteration shell shared by split and combine CLI loops.

use std::future::Future;

use serde_json::Value;

use crate::coin_ops::{coin_op_should_stop, SpendableCoin};
use crate::coin_ops::execution::CoinOpExecContext;
use crate::error::SignerResult;

use super::context::spendable_coins_for_gate;

const ITERATION_SLEEP_SECS: u64 = 2;

#[derive(Debug, Clone)]
pub struct UntilReadyLoopConfig {
    pub until_ready: bool,
    pub no_wait: bool,
    pub max_iterations: i32,
    pub explicit_coin_ids: bool,
    /// When true, stop before the iteration body when the gate reports ready.
    pub stop_when_gate_ready: bool,
}

pub enum LoopIterationOutcome {
    Continue { operation: Value },
    Break {
        operation: Option<Value>,
        reason: String,
    },
    Exit(i32),
}

#[derive(Debug, Clone)]
pub enum UntilReadyCompletion {
    Completed { stop_reason: String },
    Exit(i32),
}

impl UntilReadyCompletion {
    pub fn stop_reason(&self) -> Option<&str> {
        match self {
            Self::Completed { stop_reason } => Some(stop_reason.as_str()),
            Self::Exit(_) => None,
        }
    }

    pub fn exit_code(&self) -> Option<i32> {
        match self {
            Self::Completed { .. } => None,
            Self::Exit(code) => Some(*code),
        }
    }
}

pub async fn run_until_ready_loop<G, GateReady, GateJson, RunIteration, Fut>(
    ctx: &CoinOpExecContext,
    config: UntilReadyLoopConfig,
    mut evaluate_gate: impl FnMut(&[Value]) -> Option<G>,
    gate_ready: GateReady,
    gate_to_json: GateJson,
    mut run_iteration: RunIteration,
) -> SignerResult<(Vec<Value>, UntilReadyCompletion)>
where
    GateReady: Fn(&G) -> bool,
    GateJson: Fn(&G) -> Value,
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
        let gate_json = gate.as_ref().map(|gate| gate_to_json(gate));

        if let Some(ref gate) = gate {
            if config.until_ready && config.stop_when_gate_ready && gate_ready(gate) {
                stop_reason = "ready".to_string();
                break;
            }
            let (should_stop, reason) = coin_op_should_stop(
                config.until_ready,
                Some(gate_ready(gate)),
                config.explicit_coin_ids,
                i64::from(iteration),
                i64::from(max_iterations),
            );
            if should_stop && config.until_ready {
                stop_reason = reason.to_string();
                break;
            }
        }

        match run_iteration(iteration, spendable, gate_json).await? {
            LoopIterationOutcome::Continue { operation } => {
                operations.push(operation);
            }
            LoopIterationOutcome::Break { operation, reason } => {
                if let Some(operation) = operation {
                    operations.push(operation);
                }
                stop_reason = reason;
                break;
            }
            LoopIterationOutcome::Exit(code) => {
                return Ok((operations, UntilReadyCompletion::Exit(code)));
            }
        }

        let (should_stop, reason) = coin_op_should_stop(
            config.until_ready,
            gate.as_ref().map(|gate| gate_ready(gate)),
            config.explicit_coin_ids,
            i64::from(iteration),
            i64::from(max_iterations),
        );
        if should_stop {
            stop_reason = reason.to_string();
            break;
        }
        if config.no_wait {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(ITERATION_SLEEP_SECS)).await;
    }

    Ok((
        operations,
        UntilReadyCompletion::Completed { stop_reason },
    ))
}

pub fn until_ready_exit_code(until_ready: bool, stop_reason: &str) -> i32 {
    if until_ready && stop_reason != "ready" {
        2
    } else {
        0
    }
}
