use std::collections::HashSet;

use crate::coin_ops::{
    coin_op_non_negative_u64, coin_op_target_amount_allowed, i64_to_usize,
    plan_auto_split_selection, CoinOpPlan, SpendableCoin, SplitAutoSelectPlan,
    SplitCombinePrereqPlan, SplitPlanningProfile,
};
use crate::config::usize_to_i64;

use super::items::{
    executed_item, skip_item, skip_on_signer_err, CoinOpExecItem, CoinOpSkipResult,
};
use super::COIN_OP_ERROR_PREFIX;
use crate::coin_ops::execution::{submit_combine_prereq, CoinOpExecContext};

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
    match submit_combine_prereq(ctx, &prereq.input_coin_ids).await {
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

pub(crate) async fn execute_daemon_split_plan(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
) -> (Vec<CoinOpExecItem>, u64) {
    match execute_daemon_split_plan_inner(ctx, plan).await {
        Ok(result) => result,
        Err(skip) => skip,
    }
}

async fn execute_daemon_split_plan_inner(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
) -> CoinOpSkipResult<(Vec<CoinOpExecItem>, u64)> {
    let op_type = plan.op_type.as_str();
    let op_count = plan.op_count;
    let size_base_units = plan.size_base_units;

    if op_count == 1 {
        return Ok((
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                "split_single_coin_noop_skipped",
            )],
            0,
        ));
    }

    let amount_per_coin_mojos = size_base_units.saturating_mul(ctx.base_unit_mojo_multiplier);
    let canonical_asset_id = ctx.market.base_asset.trim();
    if !coin_op_target_amount_allowed(amount_per_coin_mojos, canonical_asset_id) {
        return Ok((
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                "split_amount_below_coin_op_minimum",
            )],
            0,
        ));
    }

    let required_amount = amount_per_coin_mojos.saturating_mul(op_count);
    let initial = match ctx.list_spendable_coins().await {
        Ok(coins) => coins,
        Err(err) => {
            return Ok((
                vec![skip_item(
                    op_type,
                    size_base_units,
                    op_count,
                    format!("{COIN_OP_ERROR_PREFIX}:{err}"),
                )],
                0,
            ));
        }
    };
    if initial.is_empty() {
        return Ok((
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                "no_spendable_split_coin_available",
            )],
            0,
        ));
    }

    let mut attempted_coin_ids = HashSet::new();
    for attempt_index in 0..2 {
        let fresh = match ctx.list_spendable_coins().await {
            Ok(coins) => coins,
            Err(err) => {
                return Ok((
                    vec![skip_item(
                        op_type,
                        size_base_units,
                        op_count,
                        format!("{COIN_OP_ERROR_PREFIX}:{err}"),
                    )],
                    0,
                ));
            }
        };
        let candidate_spendable: Vec<SpendableCoin> = fresh
            .into_iter()
            .filter(|coin| {
                !attempted_coin_ids.contains(&coin.id)
                    && !ctx.watched_coin_ids.contains(&coin.id.to_ascii_lowercase())
            })
            .collect();

        let selection = plan_auto_split_selection(
            &candidate_spendable,
            required_amount,
            canonical_asset_id,
            SplitPlanningProfile::DaemonAuto,
            ctx.combine_input_cap,
            Some(attempt_index == 0),
        );

        match selection {
            SplitAutoSelectPlan::CombinePrereq(prereq) => {
                return submit_combine_prereq_for_split_inner(
                    ctx,
                    op_type,
                    size_base_units,
                    op_count,
                    &prereq,
                )
                .await;
            }
            SplitAutoSelectPlan::Skip(skip) => {
                if skip.reason == "no_spendable_split_coin_meets_required_amount" {
                    break;
                }
                return Ok((
                    vec![skip_item(op_type, size_base_units, op_count, skip.reason)],
                    0,
                ));
            }
            SplitAutoSelectPlan::Coin(selected) => {
                attempted_coin_ids.insert(selected.coin_id.clone());
                let (amount_u64, output_count, fee_mojos) = split_execution_scalars(
                    op_type,
                    size_base_units,
                    op_count,
                    amount_per_coin_mojos,
                    ctx.program.coin_ops_split_fee_mojos,
                )?;
                let output_amounts = vec![amount_u64; output_count];
                match ctx
                    .execute_mixed_split(
                        output_amounts,
                        std::slice::from_ref(&selected.coin_id),
                        fee_mojos,
                    )
                    .await
                {
                    Ok(operation_id) => {
                        return Ok((
                            vec![executed_item(
                                op_type,
                                size_base_units,
                                op_count,
                                "signer_split_submitted",
                                operation_id,
                            )],
                            1,
                        ));
                    }
                    Err(err) => {
                        let error_text = err.to_string();
                        if error_text.contains("Some selected coins are not spendable")
                            && attempt_index == 0
                        {
                            continue;
                        }
                        return Ok((
                            vec![skip_item(
                                op_type,
                                size_base_units,
                                op_count,
                                format!(
                                    "{COIN_OP_ERROR_PREFIX}:{err}:selected_coin_id={}",
                                    selected.coin_id
                                ),
                            )],
                            0,
                        ));
                    }
                }
            }
        }
    }

    Ok((
        vec![skip_item(
            op_type,
            size_base_units,
            op_count,
            "no_spendable_split_coin_meets_required_amount",
        )],
        0,
    ))
}
