use crate::coin_ops::{
    coin_op_non_negative_u64, coin_op_target_amount_allowed, i64_to_usize,
    plan_auto_combine_inputs, CoinOpPlan, CombineInputSelectionMode, SpendableCoin,
};

use super::items::{
    executed_item, skip_item, skip_on_signer_err, CoinOpExecItem, CoinOpSkipResult,
};
use super::COIN_OP_ERROR_PREFIX;
use crate::coin_ops::execution::CoinOpExecContext;
use crate::coin_ops::{combine_output_amounts, total_for_coin_ids};

pub(crate) async fn execute_daemon_combine_plan(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
) -> (Vec<CoinOpExecItem>, u64) {
    match execute_daemon_combine_plan_inner(ctx, plan).await {
        Ok(result) => result,
        Err(skip) => skip,
    }
}

struct CombineInputSelection {
    op_type: String,
    size_base_units: i64,
    op_count: i64,
    combine_input_coin_ids: Vec<String>,
    spendable: Vec<SpendableCoin>,
}

async fn prepare_daemon_combine_inputs(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
) -> CoinOpSkipResult<CombineInputSelection> {
    let op_type = plan.op_type.as_str();
    let op_count = plan.op_count;
    let size_base_units = plan.size_base_units;
    let requested_number_of_coins = op_count.max(2);
    let capped_number_of_coins = requested_number_of_coins.min(ctx.combine_input_cap);
    let target_coin_amount_mojos = size_base_units.saturating_mul(ctx.base_unit_mojo_multiplier);
    let canonical_asset_id = ctx.market.base_asset.trim();

    if !coin_op_target_amount_allowed(target_coin_amount_mojos, canonical_asset_id) {
        return Err((
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                "combine_target_amount_below_coin_op_minimum",
            )],
            0,
        ));
    }

    let spendable = match ctx.list_spendable_coins().await {
        Ok(coins) => coins,
        Err(err) => {
            return Err((
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

    let requested_count = skip_on_signer_err(
        op_type,
        size_base_units,
        op_count,
        i64_to_usize(requested_number_of_coins, "combine.op_count"),
    )?;
    let capped_count = skip_on_signer_err(
        op_type,
        size_base_units,
        op_count,
        i64_to_usize(capped_number_of_coins, "combine.capped_op_count"),
    )?;

    let combine_input_coin_ids = match plan_auto_combine_inputs(
        &spendable,
        requested_count,
        CombineInputSelectionMode::ExactAmount,
        Some(target_coin_amount_mojos),
        Some(&ctx.watched_coin_ids),
        Some(capped_count),
    ) {
        Ok(ids) => ids,
        Err(reason) => {
            return Err((
                vec![skip_item(op_type, size_base_units, op_count, reason)],
                0,
            ));
        }
    };
    if combine_input_coin_ids.len() < 2 {
        return Err((
            vec![skip_item(
                op_type,
                size_base_units,
                op_count,
                "no_spendable_combine_coin_available",
            )],
            0,
        ));
    }

    Ok(CombineInputSelection {
        op_type: op_type.to_string(),
        size_base_units,
        op_count,
        combine_input_coin_ids,
        spendable,
    })
}

async fn submit_daemon_combine_plan(
    ctx: &CoinOpExecContext,
    selection: &CombineInputSelection,
) -> CoinOpSkipResult<(Vec<CoinOpExecItem>, u64)> {
    let CombineInputSelection {
        op_type,
        size_base_units,
        op_count,
        combine_input_coin_ids,
        spendable,
    } = selection;

    let total = total_for_coin_ids(spendable, combine_input_coin_ids);
    let output_amounts = skip_on_signer_err(
        op_type,
        *size_base_units,
        *op_count,
        combine_output_amounts(total, combine_input_coin_ids.len()),
    )?;
    let fee_mojos = skip_on_signer_err(
        op_type,
        *size_base_units,
        *op_count,
        coin_op_non_negative_u64(
            ctx.program.coin_ops_combine_fee_mojos,
            "program.coin_ops_combine_fee_mojos",
        ),
    )?;

    match ctx
        .execute_mixed_split(output_amounts, combine_input_coin_ids, fee_mojos)
        .await
    {
        Ok(operation_id) => Ok((
            vec![executed_item(
                op_type,
                *size_base_units,
                *op_count,
                "signer_combine_submitted",
                operation_id,
            )],
            1,
        )),
        Err(err) => Ok((
            vec![skip_item(
                op_type,
                *size_base_units,
                *op_count,
                format!("{COIN_OP_ERROR_PREFIX}:{err}"),
            )],
            0,
        )),
    }
}

async fn execute_daemon_combine_plan_inner(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
) -> CoinOpSkipResult<(Vec<CoinOpExecItem>, u64)> {
    let selection = prepare_daemon_combine_inputs(ctx, plan).await?;
    submit_daemon_combine_plan(ctx, &selection).await
}
