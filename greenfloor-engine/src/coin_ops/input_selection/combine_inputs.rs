use std::collections::HashSet;

use super::types::CombineInputSelectionMode;
use crate::coin_ops::selection::{select_exact_amount_coin_ids, SpendableCoin};

fn normalized_exclude_ids(exclude_coin_ids: Option<&HashSet<String>>) -> HashSet<String> {
    exclude_coin_ids
        .map(|set| set.iter().map(|id| id.to_ascii_lowercase()).collect())
        .unwrap_or_default()
}

/// Plan auto combine inputs.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn plan_auto_combine_inputs(
    spendable_coins: &[SpendableCoin],
    number_of_coins: usize,
    selection_mode: CombineInputSelectionMode,
    target_amount_mojos: Option<i64>,
    exclude_coin_ids: Option<&HashSet<String>>,
    max_count: Option<usize>,
) -> Result<Vec<String>, &'static str> {
    let capped_count = max_count.map_or(number_of_coins, |max| number_of_coins.min(max));
    let excluded = normalized_exclude_ids(exclude_coin_ids);

    if selection_mode == CombineInputSelectionMode::ExactAmount {
        let amount = target_amount_mojos
            .ok_or("target_amount_mojos is required for exact-amount combine selection")?;
        return Ok(select_exact_amount_coin_ids(
            spendable_coins,
            amount,
            &excluded,
            Some(capped_count),
        ));
    }

    let mut eligible: Vec<&SpendableCoin> = spendable_coins
        .iter()
        .filter(|coin| !coin.id.is_empty() && !excluded.contains(&coin.id.to_ascii_lowercase()))
        .collect();
    eligible.sort_by_key(|coin| std::cmp::Reverse(coin.amount));
    Ok(eligible
        .iter()
        .take(capped_count)
        .map(|coin| coin.id.clone())
        .collect())
}
