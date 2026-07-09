use crate::coin_ops::{i64_to_usize, plan_exact_amount_combine_inputs, CoinOpPlan, SpendableCoin};

use super::items::{
    execute_daemon_coin_op_plan, executed_item_for_plan, plan_skip, skip_item_for_plan,
    skip_on_signer_err_for_plan, CoinOpExecItem, CoinOpSkipResult,
};
use super::prep::{
    list_spendable_coins_for_plan, unwatched_spendable, validate_plan_target_amount,
};
use super::COIN_OP_ERROR_PREFIX;
use crate::coin_ops::execution::CoinOpExecContext;

#[allow(clippy::large_futures)]
pub(crate) async fn execute_daemon_combine_plan(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
) -> (Vec<CoinOpExecItem>, u64) {
    execute_daemon_coin_op_plan(execute_daemon_combine_plan_inner(ctx, plan)).await
}
struct CombineInputSelection {
    combine_input_coin_ids: Vec<String>,
    spendable: Vec<SpendableCoin>,
}

async fn prepare_daemon_combine_inputs(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
) -> CoinOpSkipResult<CombineInputSelection> {
    let requested_number_of_coins = plan.op_count.max(2);
    let capped_number_of_coins = requested_number_of_coins.min(ctx.combine_input_cap);
    let target_coin_amount_mojos =
        validate_plan_target_amount(ctx, plan, "combine_target_amount_below_coin_op_minimum")?;

    let spendable = unwatched_spendable(ctx, list_spendable_coins_for_plan(ctx, plan).await?);
    let requested_count = skip_on_signer_err_for_plan(
        plan,
        i64_to_usize(requested_number_of_coins, "combine.op_count"),
    )?;
    let capped_count = skip_on_signer_err_for_plan(
        plan,
        i64_to_usize(capped_number_of_coins, "combine.capped_op_count"),
    )?;

    let combine_input_coin_ids = plan_exact_amount_combine_inputs(
        &spendable,
        requested_count,
        target_coin_amount_mojos,
        None,
        Some(capped_count),
    );
    if combine_input_coin_ids.len() < 2 {
        return Err(plan_skip(plan, "no_spendable_combine_coin_available"));
    }

    Ok(CombineInputSelection {
        combine_input_coin_ids,
        spendable,
    })
}

async fn submit_daemon_combine_plan(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
    selection: &CombineInputSelection,
) -> CoinOpSkipResult<(Vec<CoinOpExecItem>, u64)> {
    match ctx
        .execute_combine(
            &selection.combine_input_coin_ids,
            Some(&selection.spendable),
        )
        .await
    {
        Ok(operation_id) => Ok((
            vec![executed_item_for_plan(
                plan,
                "signer_combine_submitted",
                operation_id,
            )],
            1,
        )),
        Err(err) => Ok((
            vec![skip_item_for_plan(
                plan,
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
    submit_daemon_combine_plan(ctx, plan, &selection).await
}
