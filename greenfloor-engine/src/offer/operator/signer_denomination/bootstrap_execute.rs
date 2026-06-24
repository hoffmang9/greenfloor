use serde_json::json;

use crate::config::{ManagerProgramConfig, SignerConfig};
use crate::error::SignerResult;
use crate::offer::bootstrap::{
    bootstrap_executed_phase, BootstrapPlanOutcome, BootstrapWaitStepKind, PlannerLadderRow,
};

use super::split_submit::{submit_bootstrap_combine, submit_bootstrap_mixed_split};
use super::wait::{wait_for_bootstrap_shape_ready, BootstrapWaitConfig};
use super::{
    executed_after_split, BootstrapPhaseFailure, BootstrapPhaseResult, ExecutedAfterSplitParams,
};

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

fn bootstrap_failed(failure: BootstrapPhaseFailure) -> BootstrapPhaseResult {
    BootstrapPhaseResult::failed(failure)
}

fn bootstrap_result_from_replan(
    replanned: &BootstrapPlanOutcome,
    ctx: &BootstrapShapeContext,
    prepend_wait_events: Vec<serde_json::Value>,
) -> BootstrapPhaseResult {
    let executed = bootstrap_executed_phase(replanned);
    let mut result = BootstrapPhaseResult::from_snapshot(executed);
    result.fee_mojos = ctx.fee_mojos;
    result.fee_source.clone_from(&ctx.fee_source);
    result.fee_lookup_error.clone_from(&ctx.fee_lookup_error);
    result.wait_events = prepend_wait_events;
    result
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

    let wait = wait_for_bootstrap_shape_ready(BootstrapWaitConfig {
        network: &program.network,
        signer: signer_config,
        ctx,
        timeout_seconds: program.runtime_offer_bootstrap_wait_timeout_seconds,
        min_timeout_seconds: BOOTSTRAP_WAIT_MIN_TIMEOUT_SECONDS,
        step: BootstrapWaitStepKind::AfterCombine,
    })
    .await
    .map_err(|err| bootstrap_wait_failed(ctx, "bootstrap_combine_wait_failed", err))?;

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
) -> Option<BootstrapPhaseResult> {
    let BootstrapPlanOutcome::NeedsShape(split_plan) = replanned else {
        return Some(bootstrap_result_from_replan(
            &replanned,
            ctx,
            prepend_wait_events,
        ));
    };
    if split_plan.requires_combine_first() {
        ctx.bootstrap_plan = split_plan;
        return Some(bootstrap_result_from_replan(
            &BootstrapPlanOutcome::NeedsShape(ctx.bootstrap_plan.clone()),
            ctx,
            prepend_wait_events,
        ));
    }
    ctx.bootstrap_plan = split_plan;
    None
}

pub(super) async fn execute_bootstrap_shape(
    program: &ManagerProgramConfig,
    signer_config: &SignerConfig,
    mut ctx: BootstrapShapeContext,
) -> SignerResult<BootstrapPhaseResult> {
    let mut prepend_wait_events = Vec::new();

    if ctx.bootstrap_plan.requires_combine_first() {
        let (events, replanned) =
            match execute_bootstrap_combine_step(program, signer_config, &ctx).await {
                Ok(result) => result,
                Err(result) => return Ok(result),
            };
        prepend_wait_events = events;
        if let Some(result) = replan_after_combine(&mut ctx, prepend_wait_events.clone(), replanned)
        {
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

    let wait = match wait_for_bootstrap_shape_ready(BootstrapWaitConfig {
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
