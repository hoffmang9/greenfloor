use std::collections::HashSet;

use crate::coin_ops::execution::CoinOpExecContext;
use crate::coin_ops::{
    coin_op_non_negative_u64, defer_low_watermark_split_from_spendable, i64_to_usize,
    plan_daemon_auto_split_selection, plan_daemon_low_watermark_split, usize_to_i64, CoinOpPlan,
    CoinOpPlanReason, DaemonAutoSplitParams, SpendableCoin, SplitAutoSelectPlan,
    SplitCombinePrereqPlan, SplitSkipReason, SplitSourceProtection,
};

use super::items::{
    execute_daemon_coin_op_plan, executed_item, executed_item_for_plan, plan_skip,
    skip_item_for_plan, skip_on_signer_err_for_plan, CoinOpExecItem, CoinOpSkipResult, PlanSkip,
};
use super::prep::{
    list_spendable_coins_for_plan, skip_if_spendable_empty, unwatched_spendable,
    validate_plan_target_amount,
};
use super::COIN_OP_ERROR_PREFIX;

fn split_execution_scalars(
    plan: &CoinOpPlan,
    amount_per_coin_mojos: i64,
    split_fee_mojos_config: i64,
) -> CoinOpSkipResult<(u64, usize, u64)> {
    let amount_u64 = skip_on_signer_err_for_plan(
        plan,
        coin_op_non_negative_u64(amount_per_coin_mojos, "split.amount_per_coin_mojos"),
    )?;
    let output_count =
        skip_on_signer_err_for_plan(plan, i64_to_usize(plan.op_count, "split.op_count"))?;
    let fee_mojos = skip_on_signer_err_for_plan(
        plan,
        coin_op_non_negative_u64(split_fee_mojos_config, "program.coin_ops_split_fee_mojos"),
    )?;
    Ok((amount_u64, output_count, fee_mojos))
}

fn low_watermark_split_protection(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
    spendable: &[SpendableCoin],
) -> Option<SplitSourceProtection> {
    if plan.reason != CoinOpPlanReason::LowWatermarkBufferDeficit {
        return None;
    }
    let sell_ladder = ctx.gated.market_row.ladders.get("sell")?;
    if sell_ladder.is_empty() {
        return None;
    }
    Some(SplitSourceProtection::from_sell_ladder_entries(
        sell_ladder,
        spendable,
        ctx.base_unit_mojo_multiplier,
    ))
}

enum SplitAttemptFlow {
    Executed(Vec<CoinOpExecItem>),
    Skipped(Vec<CoinOpExecItem>),
    Retry(String),
    NoMatch,
}

async fn submit_combine_prereq_for_split(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
    prereq: &SplitCombinePrereqPlan,
) -> Result<SplitAttemptFlow, PlanSkip> {
    let combine_count = skip_on_signer_err_for_plan(
        plan,
        usize_to_i64(prereq.input_coin_ids.len(), "split_prereq.input_count"),
    )?;
    match ctx.execute_combine(&prereq.input_coin_ids, None).await {
        Ok(operation_id) => Ok(SplitAttemptFlow::Executed(vec![executed_item(
            "combine",
            plan.size_base_units,
            combine_count,
            if prereq.exact_match {
                "signer_combine_submitted_for_split_prereq_exact"
            } else {
                "signer_combine_submitted_for_split_prereq_with_change"
            },
            operation_id,
        )])),
        Err(err) => Ok(SplitAttemptFlow::Skipped(vec![skip_item_for_plan(
            plan,
            format!("{COIN_OP_ERROR_PREFIX}:{err}:combine_for_split_prereq"),
        )])),
    }
}

async fn split_candidate_spendable(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
    attempted_coin_ids: &HashSet<String>,
    prefetched_spendable: Option<&[SpendableCoin]>,
) -> CoinOpSkipResult<Vec<SpendableCoin>> {
    let fresh =
        if let Some(prefetched) = prefetched_spendable.filter(|_| attempted_coin_ids.is_empty()) {
            prefetched.to_vec()
        } else {
            unwatched_spendable(
                ctx,
                skip_if_spendable_empty(
                    plan,
                    list_spendable_coins_for_plan(ctx, plan).await?,
                    "no_spendable_split_coin_available",
                )?,
            )
        };
    Ok(fresh
        .into_iter()
        .filter(|coin| !attempted_coin_ids.contains(&coin.id))
        .collect())
}

async fn submit_daemon_split_for_coin(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
    amount_per_coin_mojos: i64,
    selected_coin_id: String,
    first_attempt: bool,
) -> Result<SplitAttemptFlow, PlanSkip> {
    let (amount_u64, output_count, fee_mojos) = split_execution_scalars(
        plan,
        amount_per_coin_mojos,
        ctx.gated.program.coin_ops_split_fee_mojos,
    )?;
    match ctx
        .execute_mixed_split(
            vec![amount_u64; output_count],
            std::slice::from_ref(&selected_coin_id),
            fee_mojos,
        )
        .await
    {
        Ok(operation_id) => Ok(SplitAttemptFlow::Executed(vec![executed_item_for_plan(
            plan,
            "signer_split_submitted",
            operation_id,
        )])),
        Err(err) if err.is_mixed_split_selected_coins_not_spendable() && first_attempt => {
            Ok(SplitAttemptFlow::Retry(selected_coin_id))
        }
        Err(err) => Ok(SplitAttemptFlow::Skipped(vec![skip_item_for_plan(
            plan,
            format!("{COIN_OP_ERROR_PREFIX}:{err}:selected_coin_id={selected_coin_id}"),
        )])),
    }
}

#[allow(clippy::too_many_arguments)]
async fn attempt_daemon_split(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
    amount_per_coin_mojos: i64,
    required_amount: i64,
    first_attempt: bool,
    attempted_coin_ids: &HashSet<String>,
    prefetched_spendable: Option<&[SpendableCoin]>,
    split_protection: Option<&SplitSourceProtection>,
) -> Result<SplitAttemptFlow, PlanSkip> {
    let candidate_spendable =
        split_candidate_spendable(ctx, plan, attempted_coin_ids, prefetched_spendable).await?;
    let params = DaemonAutoSplitParams {
        candidate_spendable: &candidate_spendable,
        required_amount_mojos: required_amount,
        canonical_asset_id: ctx.gated.market_row.base_asset.trim(),
        combine_input_cap: ctx.combine_input_cap,
        allow_combine_prereq: first_attempt,
    };
    let selection = if let Some(protection) = split_protection {
        plan_daemon_low_watermark_split(&params, protection)
    } else {
        plan_daemon_auto_split_selection(&params)
    };

    match selection {
        SplitAutoSelectPlan::CombinePrereq(prereq) => {
            submit_combine_prereq_for_split(ctx, plan, &prereq).await
        }
        SplitAutoSelectPlan::Skip(SplitSkipReason::NoSpendableMeetsRequired) => {
            Ok(SplitAttemptFlow::NoMatch)
        }
        SplitAutoSelectPlan::Skip(reason) => {
            Ok(SplitAttemptFlow::Skipped(vec![skip_item_for_plan(
                plan,
                reason.as_str(),
            )]))
        }
        SplitAutoSelectPlan::Coin(selected) => {
            submit_daemon_split_for_coin(
                ctx,
                plan,
                amount_per_coin_mojos,
                selected.coin_id,
                first_attempt,
            )
            .await
        }
    }
}

#[allow(clippy::large_futures)]
pub(crate) async fn execute_daemon_split_plan(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
) -> (Vec<CoinOpExecItem>, u64) {
    execute_daemon_coin_op_plan(execute_daemon_split_plan_inner(ctx, plan)).await
}

#[allow(clippy::large_futures)]
async fn execute_daemon_split_plan_inner(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
) -> CoinOpSkipResult<(Vec<CoinOpExecItem>, u64)> {
    let amount_per_coin_mojos =
        validate_plan_target_amount(ctx, plan, "split_amount_below_coin_op_minimum")?;
    let required_amount = amount_per_coin_mojos.saturating_mul(plan.op_count);
    let spendable = unwatched_spendable(
        ctx,
        skip_if_spendable_empty(
            plan,
            list_spendable_coins_for_plan(ctx, plan).await?,
            "no_spendable_split_coin_available",
        )?,
    );
    if defer_low_watermark_split_from_spendable(plan, &spendable, ctx.base_unit_mojo_multiplier) {
        return Err(plan_skip(plan, "bootstrap_primary_shape_deferred"));
    }
    let split_protection = low_watermark_split_protection(ctx, plan, &spendable);
    let protection_ref = split_protection.as_ref();
    let prefetched = spendable.as_slice();
    let mut attempted_coin_ids = HashSet::new();

    for first_attempt in [true, false] {
        match attempt_daemon_split(
            ctx,
            plan,
            amount_per_coin_mojos,
            required_amount,
            first_attempt,
            &attempted_coin_ids,
            if first_attempt {
                Some(prefetched)
            } else {
                None
            },
            protection_ref,
        )
        .await
        {
            Ok(SplitAttemptFlow::Executed(items)) => return Ok((items, 1)),
            Ok(SplitAttemptFlow::Skipped(items)) => return Ok((items, 0)),
            Ok(SplitAttemptFlow::Retry(coin_id)) => {
                attempted_coin_ids.insert(coin_id);
            }
            Ok(SplitAttemptFlow::NoMatch) => break,
            Err(skip) => return Err(skip),
        }
    }

    Err(plan_skip(
        plan,
        SplitSkipReason::NoSpendableMeetsRequired.as_str(),
    ))
}
