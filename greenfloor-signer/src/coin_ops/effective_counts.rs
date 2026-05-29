use std::collections::BTreeMap;

use super::plan::BucketSpec;

pub fn effective_sell_bucket_counts_for_coin_ops(
    sell_ladder: &[BucketSpec],
    wallet_bucket_counts: &BTreeMap<i64, i64>,
    active_sell_offer_counts_by_size: Option<&BTreeMap<i64, i64>>,
    newly_executed_sell_offer_counts_by_size: Option<&BTreeMap<i64, i64>>,
) -> BTreeMap<i64, i64> {
    let empty = BTreeMap::new();
    let active_sell_counts = active_sell_offer_counts_by_size.unwrap_or(&empty);
    let newly_executed_sell_counts = newly_executed_sell_offer_counts_by_size.unwrap_or(&empty);
    let mut effective_counts = wallet_bucket_counts.clone();

    for entry in sell_ladder {
        let size_base_units = entry.size_base_units;
        if size_base_units <= 0 {
            continue;
        }
        let target_count = entry.target_count.max(0);
        let newly_executed_sell_count = newly_executed_sell_counts
            .get(&size_base_units)
            .copied()
            .unwrap_or(0)
            .max(0);
        let wallet_count = wallet_bucket_counts
            .get(&size_base_units)
            .copied()
            .unwrap_or(0)
            .max(0)
            .saturating_sub(newly_executed_sell_count);
        let active_sell_count = active_sell_counts
            .get(&size_base_units)
            .copied()
            .unwrap_or(0)
            .max(0);
        let effective_active_sell_count = active_sell_count + newly_executed_sell_count;
        // Count live sell offers toward the market target, but not toward the
        // split buffer. That preserves at most one extra ready coin above the
        // active sell ladder coverage.
        effective_counts.insert(
            size_base_units,
            wallet_count + effective_active_sell_count.min(target_count),
        );
    }
    effective_counts
}

#[cfg(test)]
mod tests {
    use super::effective_sell_bucket_counts_for_coin_ops;
    use crate::coin_ops::BucketSpec;
    use std::collections::BTreeMap;

    fn sell_ladder(entries: &[(i64, i64)]) -> Vec<BucketSpec> {
        entries
            .iter()
            .map(|(size, target)| BucketSpec {
                size_base_units: *size,
                target_count: *target,
                split_buffer_count: 0,
                combine_when_excess_factor: 0.0,
                current_count: 0,
            })
            .collect()
    }

    #[test]
    fn effective_counts_live_sells_toward_target_only() {
        let got = effective_sell_bucket_counts_for_coin_ops(
            &sell_ladder(&[(10, 3)]),
            &BTreeMap::from([(10, 0)]),
            Some(&BTreeMap::from([(10, 3)])),
            None,
        );
        assert_eq!(got.get(&10), Some(&3));
    }

    #[test]
    fn effective_counts_caps_live_sell_credit_at_target() {
        let got = effective_sell_bucket_counts_for_coin_ops(
            &sell_ladder(&[(10, 3)]),
            &BTreeMap::from([(10, 0)]),
            Some(&BTreeMap::from([(10, 4)])),
            None,
        );
        assert_eq!(got.get(&10), Some(&3));
    }

    #[test]
    fn effective_counts_accounts_for_new_sell_posts_in_cycle() {
        let got = effective_sell_bucket_counts_for_coin_ops(
            &sell_ladder(&[(10, 2)]),
            &BTreeMap::from([(10, 2)]),
            Some(&BTreeMap::from([(10, 0)])),
            Some(&BTreeMap::from([(10, 2)])),
        );
        assert_eq!(got.get(&10), Some(&2));
    }
}
