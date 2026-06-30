use crate::coin_ops::execution::CoinOpExecContext;
use crate::coin_ops::{coin_op_target_amount_allowed, CoinOpPlan, SpendableCoin};

use super::items::{plan_skip, skip_on_signer_err_for_plan, CoinOpSkipResult};

pub(crate) fn plan_target_mojos(ctx: &CoinOpExecContext, plan: &CoinOpPlan) -> i64 {
    plan.size_base_units
        .saturating_mul(ctx.base_unit_mojo_multiplier)
}

pub(crate) fn validate_plan_target_amount(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
    below_minimum_reason: &'static str,
) -> CoinOpSkipResult<i64> {
    let target_mojos = plan_target_mojos(ctx, plan);
    if !coin_op_target_amount_allowed(target_mojos, ctx.gated.market_row.base_asset.trim()) {
        return Err(plan_skip(plan, below_minimum_reason));
    }
    Ok(target_mojos)
}

pub(crate) async fn list_spendable_coins_for_plan(
    ctx: &CoinOpExecContext,
    plan: &CoinOpPlan,
) -> CoinOpSkipResult<Vec<SpendableCoin>> {
    skip_on_signer_err_for_plan(plan, ctx.list_spendable_coins().await)
}

pub(crate) fn skip_if_spendable_empty(
    plan: &CoinOpPlan,
    coins: Vec<SpendableCoin>,
    empty_reason: &'static str,
) -> CoinOpSkipResult<Vec<SpendableCoin>> {
    if coins.is_empty() {
        return Err(plan_skip(plan, empty_reason));
    }
    Ok(coins)
}

pub(crate) fn unwatched_spendable(
    ctx: &CoinOpExecContext,
    coins: impl IntoIterator<Item = SpendableCoin>,
) -> Vec<SpendableCoin> {
    coins
        .into_iter()
        .filter(|coin| !ctx.watched_coin_ids.contains(&coin.id.to_ascii_lowercase()))
        .collect()
}
