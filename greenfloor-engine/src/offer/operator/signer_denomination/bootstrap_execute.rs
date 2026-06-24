use serde_json::json;

use crate::config::{ManagerProgramConfig, SignerConfig};
use crate::error::{SignerError, SignerResult};
use crate::offer::bootstrap::{
    bootstrap_executed_phase, bootstrap_replan_after_combine, BootstrapPhaseSnapshot,
    BootstrapPlanOutcome, BootstrapReplanAfterCombine, BootstrapWaitStepKind, PlannerLadderRow,
};

use super::executed_after_split;
use super::split_submit::{submit_bootstrap_combine, submit_bootstrap_mixed_split};
use super::types::{
    BootstrapExecutedExtras, BootstrapExecutionMetadata, BootstrapPhaseFailure,
    BootstrapPhaseResult,
};
use super::wait::{wait_for_bootstrap_shape_step, BootstrapWaitConfig};
use super::ExecutedAfterSplitParams;

const BOOTSTRAP_WAIT_MIN_TIMEOUT_SECONDS: u64 = 10;

pub(crate) struct BootstrapShapeContext {
    pub(crate) split_asset_id: String,
    pub(crate) split_asset_mojo_multiplier: i64,
    pub(crate) receive_address: String,
    pub(crate) bootstrap_plan: crate::offer::bootstrap::BootstrapPlan,
    pub(crate) ladder_entries: Vec<PlannerLadderRow>,
    pub(crate) combine_context: crate::offer::bootstrap::BootstrapCombineContext,
    pub(crate) fee_mojos: u64,
    pub(crate) fee_source: String,
    pub(crate) fee_lookup_error: Option<String>,
    #[cfg(test)]
    pub(crate) test_overrides: super::test_overrides::SignerDenominationTestOverrides,
}

impl BootstrapShapeContext {
    fn execution_metadata(&self) -> BootstrapExecutionMetadata {
        BootstrapExecutionMetadata {
            fee_mojos: self.fee_mojos,
            fee_source: self.fee_source.clone(),
            fee_lookup_error: self.fee_lookup_error.clone(),
        }
    }

    fn executed_result(
        &self,
        snapshot: BootstrapPhaseSnapshot,
        extras: BootstrapExecutedExtras,
    ) -> BootstrapPhaseResult {
        BootstrapPhaseResult::from_executed(self.execution_metadata(), snapshot, extras)
    }

    fn executed_on_shape_wait_timeout(
        &self,
        reason: &'static str,
        extras: BootstrapExecutedExtras,
    ) -> BootstrapPhaseResult {
        self.executed_result(
            BootstrapPhaseSnapshot {
                status: "executed",
                reason: reason.to_string(),
                ready: false,
            },
            extras,
        )
    }

    fn executed_from_outcome(
        &self,
        outcome: &BootstrapPlanOutcome,
        wait_events: Vec<serde_json::Value>,
    ) -> BootstrapPhaseResult {
        self.executed_result(
            bootstrap_executed_phase(outcome),
            BootstrapExecutedExtras {
                wait_events,
                ..BootstrapExecutedExtras::empty()
            },
        )
    }
}

fn bootstrap_failed(failure: BootstrapPhaseFailure) -> BootstrapPhaseResult {
    BootstrapPhaseResult::failed(failure)
}

fn bootstrap_wait_failed(
    ctx: &BootstrapShapeContext,
    failure_reason: &'static str,
    err: impl std::fmt::Display,
) -> BootstrapPhaseResult {
    bootstrap_failed(
        BootstrapPhaseFailure::new(
            failure_reason,
            ctx.fee_mojos,
            ctx.fee_source.clone(),
            ctx.fee_lookup_error.clone(),
        )
        .with_plan(ctx.bootstrap_plan.clone())
        .with_wait_error(err.to_string()),
    )
}

async fn execute_bootstrap_combine_step(
    program: &ManagerProgramConfig,
    signer_config: &SignerConfig,
    ctx: &BootstrapShapeContext,
) -> Result<(Vec<serde_json::Value>, BootstrapPlanOutcome), BootstrapPhaseResult> {
    let combine_result = submit_bootstrap_combine(
        signer_config,
        &ctx.bootstrap_plan,
        &ctx.split_asset_id,
        &ctx.receive_address,
        ctx.split_asset_mojo_multiplier,
        #[cfg(test)]
        Some(&ctx.test_overrides),
    )
    .await
    .map_err(|err| {
        bootstrap_failed(BootstrapPhaseFailure::new(
            format!("signer_bootstrap_combine_error:{err}"),
            ctx.fee_mojos,
            ctx.fee_source.clone(),
            ctx.fee_lookup_error.clone(),
        ))
    })?;

    let wait = wait_for_bootstrap_shape_step(BootstrapWaitConfig {
        network: &program.network,
        signer: signer_config,
        ctx,
        timeout_seconds: program.runtime_offer_bootstrap_wait_timeout_seconds,
        min_timeout_seconds: BOOTSTRAP_WAIT_MIN_TIMEOUT_SECONDS,
        step: BootstrapWaitStepKind::AfterCombine,
    })
    .await
    .map_err(|err| {
        if matches!(err, SignerError::BootstrapShapeWaitTimeout) {
            return ctx.executed_on_shape_wait_timeout(
                "bootstrap_submitted:after_combine_wait_timeout",
                BootstrapExecutedExtras {
                    wait_events: vec![json!({
                        "event": "bootstrap_combine_submitted",
                        "combine_result": combine_result,
                    })],
                    plan: Some(ctx.bootstrap_plan.clone()),
                    wait_error: Some(err.to_string()),
                    ..BootstrapExecutedExtras::empty()
                },
            );
        }
        bootstrap_wait_failed(ctx, "bootstrap_combine_wait_failed", err)
    })?;

    let mut wait_events = wait.events;
    wait_events.insert(
        0,
        json!({
            "event": "bootstrap_combine_submitted",
            "combine_result": combine_result,
        }),
    );
    Ok((wait_events, wait.outcome))
}

#[must_use]
pub(crate) fn replan_after_combine(
    ctx: &mut BootstrapShapeContext,
    prepend_wait_events: Vec<serde_json::Value>,
    replanned: BootstrapPlanOutcome,
    combine_target_amount: i64,
) -> Option<BootstrapPhaseResult> {
    match bootstrap_replan_after_combine(combine_target_amount, replanned) {
        BootstrapReplanAfterCombine::Complete(outcome) => {
            Some(ctx.executed_from_outcome(&outcome, prepend_wait_events))
        }
        BootstrapReplanAfterCombine::ContinueSplit(plan) => {
            ctx.bootstrap_plan = plan;
            None
        }
    }
}

pub(super) async fn execute_bootstrap_shape(
    program: &ManagerProgramConfig,
    signer_config: &SignerConfig,
    mut ctx: BootstrapShapeContext,
) -> SignerResult<BootstrapPhaseResult> {
    let mut prepend_wait_events = Vec::new();

    if ctx.bootstrap_plan.requires_combine_first() {
        let combine_target_amount = ctx.bootstrap_plan.total_output_amount;
        let (events, replanned) =
            match execute_bootstrap_combine_step(program, signer_config, &ctx).await {
                Ok(result) => result,
                Err(result) => return Ok(result),
            };
        prepend_wait_events = events;
        if let Some(result) = replan_after_combine(
            &mut ctx,
            prepend_wait_events.clone(),
            replanned,
            combine_target_amount,
        ) {
            return Ok(result);
        }
    }

    let bootstrap_plan = ctx.bootstrap_plan.clone();
    let split_result = match submit_bootstrap_mixed_split(
        signer_config,
        &bootstrap_plan,
        &ctx.split_asset_id,
        &ctx.receive_address,
        ctx.split_asset_mojo_multiplier,
        #[cfg(test)]
        Some(&ctx.test_overrides),
    )
    .await
    {
        Ok(result) => result,
        Err(err) => {
            return Ok(bootstrap_failed(
                BootstrapPhaseFailure::new(
                    format!("signer_mixed_split_error:{err}"),
                    ctx.fee_mojos,
                    ctx.fee_source.clone(),
                    ctx.fee_lookup_error.clone(),
                )
                .with_plan(bootstrap_plan),
            ));
        }
    };

    let wait = match wait_for_bootstrap_shape_step(BootstrapWaitConfig {
        network: &program.network,
        signer: signer_config,
        ctx: &ctx,
        timeout_seconds: program.runtime_offer_bootstrap_wait_timeout_seconds,
        min_timeout_seconds: BOOTSTRAP_WAIT_MIN_TIMEOUT_SECONDS,
        step: BootstrapWaitStepKind::AfterSplit,
    })
    .await
    {
        Ok(wait) => wait,
        Err(err) => {
            if matches!(err, SignerError::BootstrapShapeWaitTimeout) {
                return Ok(ctx.executed_on_shape_wait_timeout(
                    "bootstrap_submitted:after_split_wait_timeout",
                    BootstrapExecutedExtras {
                        wait_events: prepend_wait_events,
                        split_result,
                        plan: Some(bootstrap_plan),
                        wait_error: Some(err.to_string()),
                    },
                ));
            }
            let mut failure = bootstrap_failed(
                BootstrapPhaseFailure::new(
                    "bootstrap_wait_failed",
                    ctx.fee_mojos,
                    ctx.fee_source.clone(),
                    ctx.fee_lookup_error.clone(),
                )
                .with_plan(bootstrap_plan)
                .with_wait_error(err.to_string()),
            );
            failure.split_result = split_result;
            return Ok(failure);
        }
    };
    let mut wait_events = wait.events;
    wait_events.splice(0..0, prepend_wait_events);

    Ok(executed_after_split(ExecutedAfterSplitParams {
        fee_mojos: ctx.fee_mojos,
        fee_source: ctx.fee_source,
        fee_lookup_error: ctx.fee_lookup_error,
        split_result,
        wait_events,
        bootstrap_plan,
        remaining: wait.outcome,
    }))
}

#[cfg(test)]
mod tests;
