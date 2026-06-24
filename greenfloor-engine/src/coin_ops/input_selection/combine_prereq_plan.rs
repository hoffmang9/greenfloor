use super::combine_selection::select_combine_inputs_for_target;
use super::types::SplitCombinePrereqPlan;
use crate::coin_ops::selection::SpendableCoin;

/// Build a daemon combine-first input set covering `target_amount_mojos`.
///
/// All amounts are on-chain **mojos** (wallet coin-op paths).
#[must_use]
pub fn build_combine_prereq_plan(
    candidate_spendable: &[SpendableCoin],
    target_amount_mojos: i64,
    combine_input_cap: i64,
) -> Option<SplitCombinePrereqPlan> {
    select_combine_inputs_for_target(candidate_spendable, target_amount_mojos, combine_input_cap)
        .map(|selection| SplitCombinePrereqPlan {
            input_coin_ids: selection.input_coin_ids,
            target_amount_mojos: selection.target_amount,
            selected_total_mojos: selection.selected_total,
            exact_match: selection.exact_match,
            cap_applied: selection.cap_applied,
            selected_count_before_cap: selection.selected_count_before_cap,
            combine_input_cap: selection.combine_input_cap,
        })
}
