use std::collections::BTreeMap;

pub fn compute_bucket_counts_from_coins(
    coin_amounts_base_units: &[i64],
    ladder_sizes: &[i64],
) -> BTreeMap<i64, i64> {
    let ladder: std::collections::BTreeSet<i64> = ladder_sizes.iter().copied().collect();
    let mut counts: BTreeMap<i64, i64> = ladder_sizes.iter().map(|size| (*size, 0)).collect();
    for amount in coin_amounts_base_units {
        if ladder.contains(amount) {
            *counts.entry(*amount).or_insert(0) += 1;
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::compute_bucket_counts_from_coins;

    #[test]
    fn bucket_counts_exact_matches_only() {
        let got = compute_bucket_counts_from_coins(
            &[1, 1, 2, 10, 100, 99],
            &[1, 10, 100],
        );
        assert_eq!(got.get(&1), Some(&2));
        assert_eq!(got.get(&10), Some(&1));
        assert_eq!(got.get(&100), Some(&1));
    }
}
