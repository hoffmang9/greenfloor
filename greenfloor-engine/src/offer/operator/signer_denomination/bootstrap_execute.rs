use std::collections::HashSet;

use serde_json::json;

use crate::coin_ops::execution::resolve_combine_input_cap;
use crate::coinset::list_wallet_unspent_coins_for_signer;
use crate::config::{ManagerProgramConfig, SignerConfig};
use crate::error::SignerResult;
use crate::offer::bootstrap::{
    bootstrap_executed_phase, plan_bootstrap_mixed_outputs, BootstrapCombineContext, BootstrapPlan,
    BootstrapPlanOutcome, PlannerLadderRow,
};

use super::planning::bootstrap_coins_in_base_units;
use super::split_submit::{submit_bootstrap_combine, submit_bootstrap_mixed_split};
use super::wait::{wait_for_coinset_confirmation, BootstrapWaitConfig};
use super::{
    executed_after_split, BootstrapPhaseFailure, BootstrapPhaseResult, ExecutedAfterSplitParams,
};

const BOOTSTRAP_WAIT_MIN_TIMEOUT_SECONDS: u64 = 10;

pub(crate) struct BootstrapShapeContext {
    pub(crate) split_asset_id: String,
    pub(crate) split_asset_mojo_multiplier: i64,
    pub(crate) receive_address: String,
    pub(crate) bootstrap_plan: BootstrapPlan,
    pub(crate) ladder_entries: Vec<PlannerLadderRow>,
    pub(crate) fee_mojos: u64,
    pub(crate) fee_source: String,
    pub(crate) fee_lookup_error: Option<String>,
    pub(crate) existing_coin_ids: HashSet<String>,
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

async fn wait_for_bootstrap_shape_confirmation(
    program: &ManagerProgramConfig,
    signer_config: &SignerConfig,
    ctx: &BootstrapShapeContext,
    failure_reason: &'static str,
) -> Result<Vec<serde_json::Value>, BootstrapPhaseResult> {
    wait_for_coinset_confirmation(BootstrapWaitConfig {
        network: &program.network,
        signer: signer_config,
        receive_address: &ctx.receive_address,
        asset_id: &ctx.split_asset_id,
        initial_coin_ids: &ctx.existing_coin_ids,
        timeout_seconds: program.runtime_offer_bootstrap_wait_timeout_seconds,
        min_timeout_seconds: BOOTSTRAP_WAIT_MIN_TIMEOUT_SECONDS,
    })
    .await
    .map_err(|err| {
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
    })
}

async fn refresh_bootstrap_spendable(
    program: &ManagerProgramConfig,
    signer_config: &SignerConfig,
    ctx: &BootstrapShapeContext,
) -> SignerResult<(
    Vec<crate::coinset::WalletUnspentCoin>,
    Vec<crate::offer::bootstrap::BootstrapCoin>,
)> {
    let asset_coins = list_wallet_unspent_coins_for_signer(
        &program.network,
        signer_config,
        &ctx.receive_address,
        &ctx.split_asset_id,
    )
    .await?;
    let spendable = bootstrap_coins_in_base_units(&asset_coins, ctx.split_asset_mojo_multiplier);
    Ok((asset_coins, spendable))
}

async fn execute_bootstrap_combine_step(
    program: &ManagerProgramConfig,
    signer_config: &SignerConfig,
    ctx: &BootstrapShapeContext,
) -> Result<Vec<serde_json::Value>, BootstrapPhaseResult> {
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

    let mut wait_events = wait_for_bootstrap_shape_confirmation(
        program,
        signer_config,
        ctx,
        "bootstrap_combine_wait_failed",
    )
    .await?;
    wait_events.insert(
        0,
        json!({
            "event": "bootstrap_combine_submitted",
            "combine_result": combine_result,
        }),
    );
    Ok(wait_events)
}

async fn replan_after_combine(
    program: &ManagerProgramConfig,
    signer_config: &SignerConfig,
    ctx: &mut BootstrapShapeContext,
    prepend_wait_events: Vec<serde_json::Value>,
) -> SignerResult<Option<BootstrapPhaseResult>> {
    let (refreshed_asset_coins, refreshed_spendable) =
        refresh_bootstrap_spendable(program, signer_config, ctx).await?;
    ctx.existing_coin_ids = refreshed_asset_coins
        .iter()
        .map(|coin| coin.id.clone())
        .collect();

    let replanned = plan_bootstrap_mixed_outputs(
        &ctx.ladder_entries,
        &refreshed_spendable,
        resolve_combine_input_cap(),
        &BootstrapCombineContext {
            mojo_multiplier: ctx.split_asset_mojo_multiplier,
            canonical_asset_id: ctx.split_asset_id.clone(),
        },
    );
    let BootstrapPlanOutcome::NeedsShape(split_plan) = replanned else {
        return Ok(Some(bootstrap_result_from_replan(
            &replanned,
            ctx,
            prepend_wait_events,
        )));
    };
    if split_plan.requires_combine_first() {
        return Ok(Some(bootstrap_result_from_replan(
            &BootstrapPlanOutcome::NeedsShape(split_plan),
            ctx,
            prepend_wait_events,
        )));
    }
    ctx.bootstrap_plan = split_plan;
    Ok(None)
}

pub(super) async fn execute_bootstrap_shape(
    program: &ManagerProgramConfig,
    signer_config: &SignerConfig,
    mut ctx: BootstrapShapeContext,
) -> SignerResult<BootstrapPhaseResult> {
    let mut prepend_wait_events = Vec::new();

    if ctx.bootstrap_plan.requires_combine_first() {
        prepend_wait_events =
            match execute_bootstrap_combine_step(program, signer_config, &ctx).await {
                Ok(events) => events,
                Err(result) => return Ok(result),
            };
        if let Some(result) = replan_after_combine(
            program,
            signer_config,
            &mut ctx,
            prepend_wait_events.clone(),
        )
        .await?
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

    let mut wait_events = match wait_for_bootstrap_shape_confirmation(
        program,
        signer_config,
        &ctx,
        "bootstrap_wait_failed",
    )
    .await
    {
        Ok(events) => events,
        Err(mut failure) => {
            failure.split_result = split_result;
            return Ok(failure);
        }
    };
    wait_events.splice(0..0, prepend_wait_events);

    let (_, refreshed_spendable) =
        refresh_bootstrap_spendable(program, signer_config, &ctx).await?;
    Ok(executed_after_split(ExecutedAfterSplitParams {
        fee_mojos: ctx.fee_mojos,
        fee_source: ctx.fee_source,
        fee_lookup_error: ctx.fee_lookup_error,
        split_result,
        wait_events,
        bootstrap_plan,
        ladder_entries: &ctx.ladder_entries,
        refreshed_spendable: &refreshed_spendable,
        combine_context: BootstrapCombineContext {
            mojo_multiplier: ctx.split_asset_mojo_multiplier,
            canonical_asset_id: ctx.split_asset_id.clone(),
        },
    }))
}

#[cfg(test)]
mod tests;
