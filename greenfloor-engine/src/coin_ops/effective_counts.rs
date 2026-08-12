use std::collections::BTreeMap;

use super::plan::LadderTargetRow;

/// Effective sell ladder coverage for coin-ops planning.
///
/// `wallet_bucket_counts` must be **free** exact clips (durable maker coin-id watches
/// already excluded by inventory). Live sells are credited toward `target_count` only.
/// Same-cycle posts still present in the pre-strategy inventory snapshot are removed via
/// `newly_executed_sell_offer_counts_by_size` before that credit is applied.
#[must_use]
pub fn effective_sell_bucket_counts_for_coin_ops(
    sell_ladder: &[LadderTargetRow],
    wallet_bucket_counts: &BTreeMap<i64, i64>,
    active_sell_offer_counts_by_size: Option<&BTreeMap<i64, i64>>,
    newly_executed_sell_offer_counts_by_size: Option<&BTreeMap<i64, i64>>,
) -> BTreeMap<i64, i64> {
    let empty = BTreeMap::default();
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
        let active_sell_count = active_sell_counts
            .get(&size_base_units)
            .copied()
            .unwrap_or(0)
            .max(0);
        // Free inventory; strip same-cycle posts still in the pre-strategy snapshot.
        // i64::saturating_sub does not clamp at zero.
        let free_wallet_count = wallet_bucket_counts
            .get(&size_base_units)
            .copied()
            .unwrap_or(0)
            .max(0)
            .saturating_sub(newly_executed_sell_count)
            .max(0);
        let reserved_clips = active_sell_count.saturating_add(newly_executed_sell_count);
        effective_counts.insert(
            size_base_units,
            free_wallet_count + reserved_clips.min(target_count),
        );
    }
    effective_counts
}

#[cfg(test)]
mod tests {
    use super::effective_sell_bucket_counts_for_coin_ops;
    use crate::coin_ops::LadderTargetRow;
    use std::collections::BTreeMap;
    fn sell_ladder(entries: &[(i64, i64)]) -> Vec<LadderTargetRow> {
        entries
            .iter()
            .map(|(size, target)| LadderTargetRow {
                size_base_units: *size,
                target_count: *target,
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

    #[test]
    fn effective_counts_free_wallet_plus_live_sell_credit() {
        // Inventory already excluded open-offer makers; wallet is free clips only.
        let got = effective_sell_bucket_counts_for_coin_ops(
            &sell_ladder(&[(10, 3)]),
            &BTreeMap::from([(10, 0)]),
            Some(&BTreeMap::from([(10, 2)])),
            None,
        );
        assert_eq!(got.get(&10), Some(&2));
    }

    #[test]
    fn effective_counts_adds_free_clips_above_live_sell_credit() {
        let got = effective_sell_bucket_counts_for_coin_ops(
            &sell_ladder(&[(10, 3)]),
            &BTreeMap::from([(10, 1)]), // one free buffer clip
            Some(&BTreeMap::from([(10, 2)])),
            None,
        );
        assert_eq!(got.get(&10), Some(&3));
    }
}
