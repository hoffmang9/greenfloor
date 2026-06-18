//! Coin amount helpers shared by daemon execution and manager CLI.

use std::collections::HashMap;

use super::scalars::non_negative_i64_to_u64_saturating;
use super::selection::SpendableCoin;

pub fn combine_output_amounts(total: i64, output_count: usize) -> Vec<u64> {
    let output_count = output_count.max(1);
    let output_count_i64 = i64::try_from(output_count).unwrap_or(i64::MAX);
    let base = total.div_euclid(output_count_i64);
    let remainder = total.rem_euclid(output_count_i64);
    let mut output_amounts = vec![non_negative_i64_to_u64_saturating(base); output_count];
    if let Some(last) = output_amounts.last_mut() {
        *last = last.saturating_add(non_negative_i64_to_u64_saturating(remainder));
    }
    output_amounts
}

pub fn total_for_coin_ids(spendable: &[SpendableCoin], coin_ids: &[String]) -> i64 {
    let amount_by_id: HashMap<String, i64> = spendable
        .iter()
        .map(|coin| (coin.id.to_ascii_lowercase(), coin.amount))
        .collect();
    coin_ids
        .iter()
        .map(|coin_id| {
            amount_by_id
                .get(&coin_id.to_ascii_lowercase())
                .copied()
                .unwrap_or(0)
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combine_output_distributes_remainder_to_last() {
        assert_eq!(combine_output_amounts(10, 3), vec![3, 3, 4]);
    }

    #[test]
    fn total_for_coin_ids_is_case_insensitive() {
        let spendable = vec![
            SpendableCoin {
                id: "Ab".to_string(),
                amount: 5,
            },
            SpendableCoin {
                id: "cd".to_string(),
                amount: 7,
            },
        ];
        assert_eq!(total_for_coin_ids(&spendable, &["ab".to_string()]), 5);
        assert_eq!(
            total_for_coin_ids(&spendable, &["AB".to_string(), "CD".to_string()]),
            12
        );
    }
}
