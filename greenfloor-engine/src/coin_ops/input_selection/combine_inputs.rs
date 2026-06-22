use std::collections::HashSet;

use crate::coin_ops::selection::{select_exact_amount_coin_ids, SpendableCoin};

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
    select_exact_amount_coin_ids(
        spendable_coins,
        amount_mojos,
        &normalized_exclude_ids(exclude_coin_ids),
        Some(capped_count(number_of_coins, max_count)),
    )
}

/// Select the largest spendable combine inputs.
#[must_use]
pub fn plan_largest_combine_inputs(
    spendable_coins: &[SpendableCoin],
    number_of_coins: usize,
    exclude_coin_ids: Option<&HashSet<String>>,
    max_count: Option<usize>,
) -> Vec<String> {
    let excluded = normalized_exclude_ids(exclude_coin_ids);
    let mut eligible: Vec<&SpendableCoin> = spendable_coins
        .iter()
        .filter(|coin| !coin.id.is_empty() && !excluded.contains(&coin.id.to_ascii_lowercase()))
        .collect();
    eligible.sort_by_key(|coin| std::cmp::Reverse(coin.amount));
    eligible
        .iter()
        .take(capped_count(number_of_coins, max_count))
        .map(|coin| coin.id.clone())
        .collect()
}
