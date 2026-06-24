use std::collections::HashSet;

use crate::coin_ops::{
    coin_op_non_negative_u64, coin_op_target_amount_allowed,
    defer_low_watermark_split_from_spendable, i64_to_usize, plan_daemon_auto_split_selection,
    plan_daemon_low_watermark_split, usize_to_i64, CoinOpPlan, CoinOpPlanReason,
    DaemonAutoSplitParams, SpendableCoin, SplitAutoSelectPlan, SplitCombinePrereqPlan,
    SplitSkipReason, SplitSourceProtection,
};

use super::items::{
    executed_item, skip_item, skip_on_signer_err, CoinOpExecItem, CoinOpSkipResult,
};
use super::COIN_OP_ERROR_PREFIX;
use crate::coin_ops::execution::CoinOpExecContext;

fn split_execution_scalars(
    op_type: &str,
    size_base_units: i64,
    op_count: i64,
    amount_per_coin_mojos: i64,
    split_fee_mojos_config: i64,
) -> CoinOpSkipResult<(u64, usize, u64)> {
    let amount_u64 = skip_on_signer_err(
        op_type,
        size_base_units,
        op_count,
        coin_op_non_negative_u64(amount_per_coin_mojos, "split.amount_per_coin_mojos"),
    )?;
    let output_count = skip_on_signer_err(
        op_type,
        size_base_units,
        op_count,
        i64_to_usize(op_count, "split.op_count"),
    )?;
    let fee_mojos = skip_on_signer_err(
        op_type,
        size_base_units,
        op_count,
        coin_op_non_negative_u64(split_fee_mojos_config, "program.coin_ops_split_fee_mojos"),
    )?;
    Ok((amount_u64, output_count, fee_mojos))
}

async fn submit_combine_prereq_for_split_inner(
    ctx: &CoinOpExecContext,
    op_type: &str,
    size_base_units: i64,
    op_count: i64,
    prereq: &SplitCombinePrereqPlan,
) -> CoinOpSkipResult<(Vec<CoinOpExecItem>, u64)> {
    let combine_count = skip_on_signer_err(
        op_type,
        size_base_units,
        op_count,
        usize_to_i64(prereq.input_coin_ids.len(), "split_prereq.input_count"),
    )?;
    match ctx.execute_combine(&prereq.input_coin_ids, None).await {
        Ok(operation_id) => {
            let reason = if prereq.exact_match {
                "signer_combine_submitted_for_split_prereq_exact"
            } else {
                "signer_combine_submitted_for_split_prereq_with_change"
            };
            Ok((
                vec![executed_item(
                    "combine",
                    size_base_units,
                    combine_count,
                    reason,
                    operation_id,
                )],
                1,
            ))
        }
        Err(err) => Ok((
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                format!("{COIN_OP_ERROR_PREFIX}:{err}:combine_for_split_prereq"),
            )],
            0,
        )),
    }
}

struct SplitPlanContext {
    op_type: String,
    op_count: i64,
    size_base_units: i64,
    amount_per_coin_mojos: i64,
    required_amount: i64,
    canonical_asset_id: String,
}

enum DaemonSplitPreflight {
    Deferred(Vec<CoinOpExecItem>, u64),
    Execute {
        spendable: Vec<SpendableCoin>,
        low_watermark_protection: Option<SplitSourceProtection>,
    },
}

fn low_watermark_split_protection(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
    spendable: &[SpendableCoin],
) -> Option<SplitSourceProtection> {
    if plan.reason != CoinOpPlanReason::LowWatermarkBufferDeficit {
        return None;
    }
    let sell_ladder = ctx.market.ladders.get("sell")?;
    if sell_ladder.is_empty() {
        return None;
    }
    Some(SplitSourceProtection::from_sell_ladder_entries(
        sell_ladder,
        spendable,
        ctx.base_unit_mojo_multiplier,
    ))
}

async fn daemon_split_preflight(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
) -> CoinOpSkipResult<DaemonSplitPreflight> {
    let initial = match ctx.list_spendable_coins().await {
        Ok(coins) => coins,
        Err(err) => {
            return Err((
                vec![skip_item(
                    plan.op_type.as_str(),
                    plan.size_base_units,
                    plan.op_count,
                    format!("{COIN_OP_ERROR_PREFIX}:{err}"),
                )],
                0,
            ));
        }
    };
    if initial.is_empty() {
        return Err((
            vec![skip_item(
                plan.op_type.as_str(),
                plan.size_base_units,
                plan.op_count,
                "no_spendable_split_coin_available",
            )],
            0,
        ));
    }

    let spendable_for_defer: Vec<SpendableCoin> = initial
        .iter()
        .filter(|coin| !ctx.watched_coin_ids.contains(&coin.id.to_ascii_lowercase()))
        .cloned()
        .collect();
    if defer_low_watermark_split_from_spendable(
        plan,
        &spendable_for_defer,
        ctx.base_unit_mojo_multiplier,
    ) {
        return Ok(DaemonSplitPreflight::Deferred(
            vec![skip_item(
                plan.op_type.as_str(),
                plan.size_base_units,
                plan.op_count,
                "bootstrap_primary_shape_deferred",
            )],
            0,
        ));
    }
    Ok(DaemonSplitPreflight::Execute {
        spendable: spendable_for_defer.clone(),
        low_watermark_protection: low_watermark_split_protection(ctx, plan, &spendable_for_defer),
    })
}

pub(crate) async fn execute_daemon_split_plan(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
) -> (Vec<CoinOpExecItem>, u64) {
    match daemon_split_preflight(ctx, plan).await {
        Ok(DaemonSplitPreflight::Deferred(items, executed)) => (items, executed),
        Err(skip) => skip,
        Ok(DaemonSplitPreflight::Execute {
            spendable,
            low_watermark_protection,
        }) => {
            match Box::pin(execute_daemon_split_plan_inner(
                ctx,
                plan,
                Some(spendable),
                low_watermark_protection,
            ))
            .await
            {
                Ok(result) => result,
                Err(skip) => skip,
            }
        }
    }
}

fn prepare_split_plan_context(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
) -> CoinOpSkipResult<SplitPlanContext> {
    let op_type = plan.op_type.as_str();
    let op_count = plan.op_count;
    let size_base_units = plan.size_base_units;

    let amount_per_coin_mojos = size_base_units.saturating_mul(ctx.base_unit_mojo_multiplier);
    let canonical_asset_id = ctx.market.base_asset.trim();
    if !coin_op_target_amount_allowed(amount_per_coin_mojos, canonical_asset_id) {
        return Err((
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                "split_amount_below_coin_op_minimum",
            )],
            0,
        ));
    }

    Ok(SplitPlanContext {
        op_type: op_type.to_string(),
        op_count,
        size_base_units,
        amount_per_coin_mojos,
        required_amount: amount_per_coin_mojos.saturating_mul(op_count),
        canonical_asset_id: canonical_asset_id.to_string(),
    })
}

enum DaemonSplitAttemptResult {
    Finished((Vec<CoinOpExecItem>, u64)),
    Retry(String),
    NoMatchingCoin,
}

async fn split_candidate_spendable(
    ctx: &CoinOpExecContext,
    split_ctx: &SplitPlanContext,
    attempted_coin_ids: &HashSet<String>,
    prefetched_spendable: Option<&[SpendableCoin]>,
) -> CoinOpSkipResult<Vec<SpendableCoin>> {
    let fresh =
        if let Some(prefetched) = prefetched_spendable.filter(|_| attempted_coin_ids.is_empty()) {
            prefetched.to_vec()
        } else {
            match ctx.list_spendable_coins().await {
                Ok(coins) => coins,
                Err(err) => {
                    return Err((
                        vec![skip_item(
                            &split_ctx.op_type,
                            split_ctx.size_base_units,
                            split_ctx.op_count,
                            format!("{COIN_OP_ERROR_PREFIX}:{err}"),
                        )],
                        0,
                    ));
                }
            }
        };
    Ok(fresh
        .into_iter()
        .filter(|coin| {
            !attempted_coin_ids.contains(&coin.id)
                && !ctx.watched_coin_ids.contains(&coin.id.to_ascii_lowercase())
        })
        .collect())
}

async fn submit_daemon_split_for_coin(
    ctx: &CoinOpExecContext,
    split_ctx: &SplitPlanContext,
    selected_coin_id: String,
    attempt_index: usize,
) -> CoinOpSkipResult<DaemonSplitAttemptResult> {
    let (amount_u64, output_count, fee_mojos) = split_execution_scalars(
        &split_ctx.op_type,
        split_ctx.size_base_units,
        split_ctx.op_count,
        split_ctx.amount_per_coin_mojos,
        ctx.program.coin_ops_split_fee_mojos,
    )?;
    let output_amounts = vec![amount_u64; output_count];
    match ctx
        .execute_mixed_split(
            output_amounts,
            std::slice::from_ref(&selected_coin_id),
            fee_mojos,
        )
        .await
    {
        Ok(operation_id) => Ok(DaemonSplitAttemptResult::Finished((
            vec![executed_item(
                &split_ctx.op_type,
                split_ctx.size_base_units,
                split_ctx.op_count,
                "signer_split_submitted",
                operation_id,
            )],
            1,
        ))),
        Err(err) => {
            let error_text = err.to_string();
            if error_text.contains("Some selected coins are not spendable") && attempt_index == 0 {
                Ok(DaemonSplitAttemptResult::Retry(selected_coin_id))
            } else {
                Ok(DaemonSplitAttemptResult::Finished((
                    vec![skip_item(
                        &split_ctx.op_type,
                        split_ctx.size_base_units,
                        split_ctx.op_count,
                        format!("{COIN_OP_ERROR_PREFIX}:{err}:selected_coin_id={selected_coin_id}"),
                    )],
                    0,
                )))
            }
        }
    }
}

async fn attempt_daemon_split(
    ctx: &CoinOpExecContext,
    split_ctx: &SplitPlanContext,
    attempt_index: usize,
    attempted_coin_ids: &HashSet<String>,
    prefetched_spendable: Option<&[SpendableCoin]>,
    split_protection: Option<&SplitSourceProtection>,
) -> CoinOpSkipResult<DaemonSplitAttemptResult> {
    let candidate_spendable = split_candidate_spendable(
        ctx,
        split_ctx,
        attempted_coin_ids,
        if attempt_index == 0 {
            prefetched_spendable
        } else {
            None
        },
    )
    .await?;
    let params = DaemonAutoSplitParams {
        candidate_spendable: &candidate_spendable,
        required_amount_mojos: split_ctx.required_amount,
        canonical_asset_id: &split_ctx.canonical_asset_id,
        combine_input_cap: ctx.combine_input_cap,
        allow_combine_prereq: attempt_index == 0,
    };
    let selection = if let Some(protection) = split_protection {
        plan_daemon_low_watermark_split(&params, protection)
    } else {
        plan_daemon_auto_split_selection(&params)
    };

    match selection {
        SplitAutoSelectPlan::CombinePrereq(prereq) => Ok(DaemonSplitAttemptResult::Finished(
            submit_combine_prereq_for_split_inner(
                ctx,
                &split_ctx.op_type,
                split_ctx.size_base_units,
                split_ctx.op_count,
                &prereq,
            )
            .await?,
        )),
        SplitAutoSelectPlan::Skip(reason) => {
            if matches!(reason, SplitSkipReason::NoSpendableMeetsRequired) {
                Ok(DaemonSplitAttemptResult::NoMatchingCoin)
            } else {
                Ok(DaemonSplitAttemptResult::Finished((
                    vec![skip_item(
                        &split_ctx.op_type,
                        split_ctx.size_base_units,
                        split_ctx.op_count,
                        reason.as_str(),
                    )],
                    0,
                )))
            }
        }
        SplitAutoSelectPlan::Coin(selected) => {
            submit_daemon_split_for_coin(ctx, split_ctx, selected.coin_id, attempt_index).await
        }
    }
}

async fn execute_daemon_split_plan_inner(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
    prefetched_spendable: Option<Vec<SpendableCoin>>,
    split_protection: Option<SplitSourceProtection>,
) -> CoinOpSkipResult<(Vec<CoinOpExecItem>, u64)> {
    let split_ctx = prepare_split_plan_context(ctx, plan)?;
    let prefetched = prefetched_spendable.as_deref();
    let protection_ref = split_protection.as_ref();
    let mut attempted_coin_ids = HashSet::new();
    for attempt_index in 0..2 {
        match Box::pin(attempt_daemon_split(
            ctx,
            &split_ctx,
            attempt_index,
            &attempted_coin_ids,
            prefetched,
            protection_ref,
        ))
        .await?
        {
            DaemonSplitAttemptResult::Finished(result) => return Ok(result),
            DaemonSplitAttemptResult::Retry(coin_id) => {
                attempted_coin_ids.insert(coin_id);
            }
            DaemonSplitAttemptResult::NoMatchingCoin => break,
        }
    }

    Ok((
        vec![skip_item(
            &split_ctx.op_type,
            split_ctx.size_base_units,
            split_ctx.op_count,
            SplitSkipReason::NoSpendableMeetsRequired.as_str(),
        )],
        0,
    ))
}
