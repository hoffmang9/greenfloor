use serde_json::json;

use crate::error::{SignerError, SignerResult};
use crate::offer::bootstrap::{
    bootstrap_executed_phase, bootstrap_replan_after_combine, BootstrapCoin,
    BootstrapPhaseSnapshot, BootstrapPhaseStatus, BootstrapPlanOutcome,
    BootstrapReplanAfterCombine, BootstrapWaitStepKind, PlannerLadderRow,
};

use super::executed_after_split;
use super::split_submit::{submit_bootstrap_combine, submit_bootstrap_mixed_split};
use super::types::{
    BootstrapExecutedExtras, BootstrapExecutionMetadata, BootstrapPhaseFailure,
    BootstrapPhaseResult,
};
use super::wait::{wait_for_bootstrap_shape_step, BootstrapWaitConfig};
use crate::offer::operator::build_and_post::ResolvedBuildAndPostContext;

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
                status: BootstrapPhaseStatus::Executed,
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

#[allow(clippy::large_futures)]
async fn execute_bootstrap_combine_step(
    ctx: &ResolvedBuildAndPostContext,
    shape: &BootstrapShapeContext,
) -> Result<
    (
        Vec<serde_json::Value>,
        BootstrapPlanOutcome,
        Vec<BootstrapCoin>,
    ),
    BootstrapPhaseResult,
> {
    let combine_result = submit_bootstrap_combine(
        &ctx.gated.signer,
        &ctx.gated.operator_network,
        &shape.bootstrap_plan,
        &shape.split_asset_id,
        &shape.receive_address,
        shape.split_asset_mojo_multiplier,
        #[cfg(test)]
        Some(&shape.test_overrides),
    )
    .await
    .map_err(|err| {
        bootstrap_failed(BootstrapPhaseFailure::new(
            format!("signer_bootstrap_combine_error:{err}"),
            shape.fee_mojos,
            shape.fee_source.clone(),
            shape.fee_lookup_error.clone(),
        ))
    })?;

    let wait = wait_for_bootstrap_shape_step(BootstrapWaitConfig {
        network: &ctx.gated.operator_network,
        signer: &ctx.gated.signer,
        ctx: shape,
        timeout_seconds: ctx
            .gated
            .program
            .runtime_offer_bootstrap_wait_timeout_seconds,
        min_timeout_seconds: BOOTSTRAP_WAIT_MIN_TIMEOUT_SECONDS,
        step: BootstrapWaitStepKind::AfterCombine,
        timings: super::wait::BootstrapWaitTimings::PRODUCTION,
        test_elapsed_secs: None,
    })
    .await
    .map_err(|err| {
        if matches!(err, SignerError::BootstrapShapeWaitTimeout) {
            return shape.executed_on_shape_wait_timeout(
                "bootstrap_submitted:after_combine_wait_timeout",
                BootstrapExecutedExtras {
                    wait_events: vec![json!({
                        "event": "bootstrap_combine_submitted",
                        "combine_result": combine_result,
                    })],
                    plan: Some(shape.bootstrap_plan.clone()),
                    wait_error: Some(err.to_string()),
                    ..BootstrapExecutedExtras::empty()
                },
            );
        }
        bootstrap_wait_failed(shape, "bootstrap_combine_wait_failed", err)
    })?;

    let mut wait_events = wait.events;
    wait_events.insert(
        0,
        json!({
            "event": "bootstrap_combine_submitted",
            "combine_result": combine_result,
        }),
    );
    Ok((wait_events, wait.outcome, wait.spendable_coins))
}

#[must_use]
pub(crate) fn replan_after_combine(
    ctx: &mut BootstrapShapeContext,
    prepend_wait_events: Vec<serde_json::Value>,
    replanned: BootstrapPlanOutcome,
    combine_target_amount: i64,
    spendable_coins: &[BootstrapCoin],
) -> Option<BootstrapPhaseResult> {
    match bootstrap_replan_after_combine(
        combine_target_amount,
        replanned,
        &ctx.ladder_entries,
        spendable_coins,
    ) {
        BootstrapReplanAfterCombine::Complete(outcome) => {
            Some(ctx.executed_from_outcome(&outcome, prepend_wait_events))
        }
        BootstrapReplanAfterCombine::ContinueSplit(plan) => {
            ctx.bootstrap_plan = plan;
            None
        }
    }
}

#[allow(clippy::large_futures)]
pub(super) async fn execute_bootstrap_shape(
    build_ctx: &ResolvedBuildAndPostContext,
    mut shape: BootstrapShapeContext,
) -> SignerResult<BootstrapPhaseResult> {
    let mut prepend_wait_events = Vec::new();

    if shape.bootstrap_plan.requires_combine_first() {
        let combine_target_amount = shape.bootstrap_plan.total_output_amount;
        let (events, replanned, spendable) =
            match execute_bootstrap_combine_step(build_ctx, &shape).await {
                Ok(result) => result,
                Err(result) => return Ok(result),
            };
        prepend_wait_events = events;
        if let Some(result) = replan_after_combine(
            &mut shape,
            prepend_wait_events.clone(),
            replanned,
            combine_target_amount,
            &spendable,
        ) {
            return Ok(result);
        }
    }

    let bootstrap_plan = shape.bootstrap_plan.clone();
    let split_result = match submit_bootstrap_mixed_split(
        &build_ctx.gated.signer,
        &build_ctx.gated.operator_network,
        &bootstrap_plan,
        &shape.split_asset_id,
        &shape.receive_address,
        shape.split_asset_mojo_multiplier,
        #[cfg(test)]
        Some(&shape.test_overrides),
    )
    .await
    {
        Ok(result) => result,
        Err(err) => {
            return Ok(bootstrap_failed(
                BootstrapPhaseFailure::new(
                    format!("signer_mixed_split_error:{err}"),
                    shape.fee_mojos,
                    shape.fee_source.clone(),
                    shape.fee_lookup_error.clone(),
                )
                .with_plan(bootstrap_plan),
            ));
        }
    };

    let wait = match wait_for_bootstrap_shape_step(BootstrapWaitConfig {
        network: &build_ctx.gated.operator_network,
        signer: &build_ctx.gated.signer,
        ctx: &shape,
        timeout_seconds: build_ctx
            .gated
            .program
            .runtime_offer_bootstrap_wait_timeout_seconds,
        min_timeout_seconds: BOOTSTRAP_WAIT_MIN_TIMEOUT_SECONDS,
        step: BootstrapWaitStepKind::AfterSplit,
        timings: super::wait::BootstrapWaitTimings::PRODUCTION,
        test_elapsed_secs: None,
    })
    .await
    {
        Ok(wait) => wait,
        Err(err) => {
            if matches!(err, SignerError::BootstrapShapeWaitTimeout) {
                return Ok(shape.executed_on_shape_wait_timeout(
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
                    shape.fee_mojos,
                    shape.fee_source.clone(),
                    shape.fee_lookup_error.clone(),
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

    Ok(executed_after_split(super::ExecutedAfterSplitParams {
        fee_mojos: shape.fee_mojos,
        fee_source: shape.fee_source,
        fee_lookup_error: shape.fee_lookup_error,
        split_result,
        wait_events,
        bootstrap_plan,
        remaining: wait.outcome,
    }))
}

#[cfg(test)]
mod tests;
