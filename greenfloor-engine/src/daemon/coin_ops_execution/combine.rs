use crate::coin_ops::{
    coin_op_target_amount_allowed, plan_auto_combine_inputs, CoinOpPlan, CombineInputSelectionMode,
};

use crate::coin_ops::execution::CoinOpExecContext;
use crate::coin_ops::{combine_output_amounts, total_for_coin_ids};
use super::items::{executed_item, skip_item, CoinOpExecItem};
use super::COIN_OP_ERROR_PREFIX;

pub(crate) async fn execute_daemon_combine_plan(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
) -> (Vec<CoinOpExecItem>, u64) {
    let op_type = plan.op_type.as_str();
    let op_count = plan.op_count;
    let size_base_units = plan.size_base_units;
    let requested_number_of_coins = op_count.max(2);
    let capped_number_of_coins = requested_number_of_coins.min(ctx.combine_input_cap);
    let target_coin_amount_mojos = size_base_units.saturating_mul(ctx.base_unit_mojo_multiplier);
    let canonical_asset_id = ctx.market.base_asset.trim();

    if !coin_op_target_amount_allowed(target_coin_amount_mojos, canonical_asset_id) {
        return (
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                "combine_target_amount_below_coin_op_minimum",
            )],
            0,
        );
    }

    let spendable = match ctx.list_spendable_coins().await {
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

    let combine_input_coin_ids = match plan_auto_combine_inputs(
        &spendable,
        requested_number_of_coins as usize,
        CombineInputSelectionMode::ExactAmount,
        Some(target_coin_amount_mojos),
        Some(&ctx.watched_coin_ids),
        Some(capped_number_of_coins as usize),
    ) {
        Ok(ids) => ids,
        Err(reason) => {
            return (
                vec![skip_item(op_type, size_base_units, op_count, reason)],
                0,
            );
        }
    };
    if combine_input_coin_ids.len() < 2 {
        return (
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                "no_spendable_combine_coin_available",
            )],
            0,
        );
    }

    let total = total_for_coin_ids(&spendable, &combine_input_coin_ids);
    let output_amounts = combine_output_amounts(total, combine_input_coin_ids.len());

    match ctx
        .execute_mixed_split(
            output_amounts,
            &combine_input_coin_ids,
            ctx.program.coin_ops_combine_fee_mojos.max(0) as u64,
        )
        .await
    {
        Ok(operation_id) => (
            vec![executed_item(
                op_type,
                size_base_units,
                op_count,
                "signer_combine_submitted",
                operation_id,
            )],
            1,
        ),
        Err(err) => (
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                format!("{COIN_OP_ERROR_PREFIX}:{err}"),
            )],
            0,
        ),
    }
}
