use std::collections::HashSet;

use super::types::SplitCombinePrereqPlan;
use crate::coin_ops::selection::{select_spendable_coins_for_target_amount, SpendableCoin};
use crate::metrics::metric_non_negative_usize;

#[must_use]
pub fn build_combine_prereq_plan(
    candidate_spendable: &[SpendableCoin],
    required_amount_mojos: i64,
    combine_input_cap: i64,
) -> Option<SplitCombinePrereqPlan> {
    let (combine_coin_ids, _, _) =
        select_spendable_coins_for_target_amount(candidate_spendable, required_amount_mojos);
    if combine_coin_ids.len() < 2 {
        return None;
    }

    let cap = metric_non_negative_usize(combine_input_cap);
    let combine_input_coin_ids: Vec<String> = combine_coin_ids.iter().take(cap).cloned().collect();
    if combine_input_coin_ids.len() < 2 {
        return None;
    }
    let selected_ids: HashSet<&str> = combine_input_coin_ids.iter().map(String::as_str).collect();
    let combine_selected_total: i64 = candidate_spendable
        .iter()
        .filter(|coin| selected_ids.contains(coin.id.as_str()))
        .map(|coin| coin.amount)
        .sum();

    let cap_applied = combine_input_coin_ids.len() < combine_coin_ids.len();
    Some(SplitCombinePrereqPlan {
        input_coin_ids: combine_input_coin_ids,
        target_amount: combine_selected_total.min(required_amount_mojos),
        selected_total: combine_selected_total,
        exact_match: combine_selected_total == required_amount_mojos,
        cap_applied,
        selected_count_before_cap: combine_coin_ids.len(),
        combine_input_cap,
    })
}
