use std::collections::HashSet;

use crate::coin_ops::selection::{select_funding_coin_ids, FundingSelectionMode, SpendableCoin};

fn normalized_exclude_ids(exclude_coin_ids: Option<&HashSet<String>>) -> HashSet<String> {
    exclude_coin_ids
        .map(|set| set.iter().map(|id| id.to_ascii_lowercase()).collect())
        .unwrap_or_default()
}

fn capped_count(number_of_coins: usize, max_count: Option<usize>) -> usize {
    max_count.map_or(number_of_coins, |max| number_of_coins.min(max))
}

/// Select combine inputs that each match the target denomination.
#[must_use]
pub fn plan_exact_amount_combine_inputs(
    spendable_coins: &[SpendableCoin],
    number_of_coins: usize,
    amount_mojos: i64,
    exclude_coin_ids: Option<&HashSet<String>>,
    max_count: Option<usize>,
) -> Vec<String> {
    let excluded = normalized_exclude_ids(exclude_coin_ids);
    select_funding_coin_ids(
        FundingSelectionMode::ExactDenom,
        spendable_coins,
        amount_mojos,
        Some(&excluded),
        Some(capped_count(number_of_coins, max_count)),
    )
}
