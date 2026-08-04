use super::types::SplitCombinePrereqPlan;
use crate::coin_ops::selection::SpendableCoin;
use crate::coin_ops::shape::{plan_combine_inputs_for_target, ShapeCoin};

/// Build a daemon combine-first input set covering `target_amount_mojos`.
///
/// All amounts are on-chain **mojos** (wallet coin-op paths).
#[must_use]
pub fn build_combine_prereq_plan(
    candidate_spendable: &[SpendableCoin],
    target_amount_mojos: i64,
    combine_input_cap: i64,
) -> Option<SplitCombinePrereqPlan> {
    let coins: Vec<ShapeCoin> = candidate_spendable
        .iter()
        .map(|coin| ShapeCoin::new(coin.id.clone(), coin.amount))
        .collect();
    plan_combine_inputs_for_target(&coins, target_amount_mojos, combine_input_cap)
}
