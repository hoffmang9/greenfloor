use std::collections::HashSet;

use crate::coin_ops::{
    coin_op_target_amount_allowed, plan_auto_split_selection, CoinOpPlan, SpendableCoin,
    SplitAutoSelectPlan, SplitCombinePrereqPlan, SplitPlanningProfile,
};

use super::items::{executed_item, skip_item, CoinOpExecItem};
use super::COIN_OP_ERROR_PREFIX;
use crate::coin_ops::execution::{submit_combine_prereq, CoinOpExecContext};

pub(crate) async fn submit_combine_prereq_for_split(
    ctx: &CoinOpExecContext,
    op_type: &str,
    size_base_units: i64,
    op_count: i64,
    _required_amount: i64,
    prereq: &SplitCombinePrereqPlan,
) -> (Vec<CoinOpExecItem>, u64) {
    let combine_count = prereq.input_coin_ids.len() as i64;
    match submit_combine_prereq(ctx, &prereq.input_coin_ids).await {
        Ok(operation_id) => {
            let reason = if prereq.exact_match {
                "signer_combine_submitted_for_split_prereq_exact"
            } else {
                "signer_combine_submitted_for_split_prereq_with_change"
            };
            (
                vec![executed_item(
                    "combine",
                    size_base_units,
                    combine_count,
                    reason,
                    operation_id,
                )],
                1,
            )
        }
        Err(err) => (
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                format!("{COIN_OP_ERROR_PREFIX}:{err}:combine_for_split_prereq"),
            )],
            0,
        ),
    }
}

pub(crate) async fn execute_daemon_split_plan(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
) -> (Vec<CoinOpExecItem>, u64) {
    let op_type = plan.op_type.as_str();
    let op_count = plan.op_count;
    let size_base_units = plan.size_base_units;

    if op_count == 1 {
        return (
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                "split_single_coin_noop_skipped",
            )],
            0,
        );
    }

    let amount_per_coin_mojos = size_base_units.saturating_mul(ctx.base_unit_mojo_multiplier);
    let canonical_asset_id = ctx.market.base_asset.trim();
    if !coin_op_target_amount_allowed(amount_per_coin_mojos, canonical_asset_id) {
        return (
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                "split_amount_below_coin_op_minimum",
            )],
            0,
        );
    }

    let required_amount = amount_per_coin_mojos.saturating_mul(op_count);
    let initial = match ctx.list_spendable_coins().await {
        Ok(coins) => coins,
        Err(err) => {
            return (
                vec![skip_item(
                    op_type,
                    size_base_units,
                    op_count,
                    format!("{COIN_OP_ERROR_PREFIX}:{err}"),
                )],
                0,
            );
        }
    };
    if initial.is_empty() {
        return (
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                "no_spendable_split_coin_available",
            )],
            0,
        );
    }

    let mut attempted_coin_ids = HashSet::new();
    for attempt_index in 0..2 {
        let fresh = match ctx.list_spendable_coins().await {
            Ok(coins) => coins,
            Err(err) => {
                return (
                    vec![skip_item(
                        op_type,
                        size_base_units,
                        op_count,
                        format!("{COIN_OP_ERROR_PREFIX}:{err}"),
                    )],
                    0,
                );
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
                return submit_combine_prereq_for_split(
                    ctx,
                    op_type,
                    size_base_units,
                    op_count,
                    required_amount,
                    &prereq,
                )
                .await;
            }
            SplitAutoSelectPlan::Skip(skip) => {
                if skip.reason == "no_spendable_split_coin_meets_required_amount" {
                    break;
                }
                return (
                    vec![skip_item(op_type, size_base_units, op_count, skip.reason)],
                    0,
                );
            }
            SplitAutoSelectPlan::Coin(selected) => {
                attempted_coin_ids.insert(selected.coin_id.clone());
                let output_amounts = vec![amount_per_coin_mojos.max(0) as u64; op_count as usize];
                match ctx
                    .execute_mixed_split(
                        output_amounts,
                        std::slice::from_ref(&selected.coin_id),
                        ctx.program.coin_ops_split_fee_mojos.max(0) as u64,
                    )
                    .await
                {
                    Ok(operation_id) => {
                        return (
                            vec![executed_item(
                                op_type,
                                size_base_units,
                                op_count,
                                "signer_split_submitted",
                                operation_id,
                            )],
                            1,
                        );
                    }
                    Err(err) => {
                        let error_text = err.to_string();
                        if error_text.contains("Some selected coins are not spendable")
                            && attempt_index == 0
                        {
                            continue;
                        }
                        return (
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
                        );
                    }
                }
            }
        }
    }

    (
        vec![skip_item(
            op_type,
            size_base_units,
            op_count,
            "no_spendable_split_coin_meets_required_amount",
        )],
        0,
    )
}
