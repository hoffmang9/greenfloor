//! Pure surplus reclaim planning: group expired makers and choose reclaim vs leave-for-strategy.

use std::collections::HashMap;

use crate::config::{market_ladder_target_count, market_wants_ladder_size, MarketConfig};
use crate::offer::request::effective_offer_side;
use crate::storage::ReusablePresplitMakerRow;

/// True when active open/`refresh_due` already meets or exceeds ladder `target_count`.
#[must_use]
pub(crate) fn ladder_at_capacity(target_count: i64, active_open_count: i64) -> bool {
    active_open_count.max(0) >= target_count.max(0)
}

/// Expired makers soft-expire should reclaim.
///
/// For wanted `(side, size)`: when the ladder is at capacity, reclaim every expired maker;
/// otherwise keep **all** expired makers so strategy `ensure_size_n_offer` can
/// `PreferExisting` by CONDITIONS hash. Soft-expire never posts.
#[must_use]
pub(crate) fn plan_soft_expire_reclaims(
    expired: Vec<ReusablePresplitMakerRow>,
    market: &MarketConfig,
    active_open_by_side_size: &HashMap<(String, i64), i64>,
) -> Vec<ReusablePresplitMakerRow> {
    let mut order: Vec<(String, i64)> = Vec::new();
    let mut groups: HashMap<(String, i64), Vec<ReusablePresplitMakerRow>> = HashMap::new();
    let mut reclaim = Vec::new();

    for row in expired {
        let side = effective_offer_side(row.offer_side.as_deref()).to_string();
        let Some(ladder_size) = row.size_base_units.filter(|units| *units > 0) else {
            reclaim.push(row);
            continue;
        };
        let key = (side, ladder_size);
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(row);
    }

    for key in order {
        let makers = groups.remove(&key).unwrap_or_default();
        if !market_wants_ladder_size(market, &key.0, key.1) {
            reclaim.extend(makers);
            continue;
        }
        let target = market_ladder_target_count(market, &key.0, key.1);
        let active = active_open_by_side_size.get(&key).copied().unwrap_or(0);
        if ladder_at_capacity(target, active) {
            reclaim.extend(makers);
        }
        // below capacity: leave every maker for strategy hash-match PreferExisting.
    }
    reclaim
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::config::LadderEntry;
    use crate::test_support::market_config::sample_market;

    fn maker(
        offer_id: &str,
        size_base_units: Option<i64>,
        offer_side: &str,
    ) -> ReusablePresplitMakerRow {
        ReusablePresplitMakerRow {
            offer_id: offer_id.into(),
            market_id: "m1".into(),
            state: "expired".into(),
            size_base_units,
            offer_side: Some(offer_side.into()),
            cancel_input_coin_id: format!("coin-{offer_id}"),
            fixed_delegated_puzzle_hash: "hh".into(),
            offer_nonce: Some("nn".into()),
            listing_expires_at: None,
        }
    }

    fn stable_market_with_target(size: i64, target_count: i64) -> MarketConfig {
        let mut market = sample_market("xch1test");
        market.quote_asset_type = "stable".into();
        let mut ladders = HashMap::new();
        ladders.insert(
            "sell".to_string(),
            vec![LadderEntry {
                size_base_units: size,
                target_count,
                split_buffer_count: 0,
                combine_when_excess_factor: 0.0,
            }],
        );
        market.ladders = ladders;
        market
    }

    #[test]
    fn plan_reclaims_unwanted_and_missing_size_keeps_all_when_gap() {
        let market = stable_market_with_target(10, 1);
        let reclaim = plan_soft_expire_reclaims(
            vec![
                maker("a", Some(10), "sell"),
                maker("b", Some(10), "sell"),
                maker("c", Some(99), "sell"),
                maker("d", None, "sell"),
            ],
            &market,
            &HashMap::new(),
        );
        let ids: std::collections::HashSet<_> =
            reclaim.iter().map(|row| row.offer_id.as_str()).collect();
        // gap=1 → keep both wanted makers for strategy; reclaim unwanted + missing size
        assert_eq!(ids, std::collections::HashSet::from(["c", "d"]));
    }

    #[test]
    fn plan_reclaims_all_when_gap_is_zero() {
        let market = stable_market_with_target(10, 2);
        let active = HashMap::from([(("sell".into(), 10_i64), 2_i64)]);
        let reclaim = plan_soft_expire_reclaims(
            vec![maker("a", Some(10), "sell"), maker("b", Some(10), "sell")],
            &market,
            &active,
        );
        assert_eq!(reclaim.len(), 2);
        assert_eq!(reclaim[0].offer_id, "a");
        assert_eq!(reclaim[1].offer_id, "b");
    }

    #[test]
    fn plan_keeps_all_wanted_makers_when_gap_positive() {
        let market = stable_market_with_target(10, 2);
        let active = HashMap::from([(("sell".into(), 10_i64), 1_i64)]);
        let reclaim = plan_soft_expire_reclaims(
            vec![
                maker("a", Some(10), "sell"),
                maker("b", Some(10), "sell"),
                maker("c", Some(10), "sell"),
            ],
            &market,
            &active,
        );
        // gap=1 → keep all three for hash-match selection; reclaim none at this size
        assert!(reclaim.is_empty());
    }

    #[test]
    fn ladder_at_capacity_when_active_meets_target() {
        assert!(!ladder_at_capacity(2, 0));
        assert!(!ladder_at_capacity(2, 1));
        assert!(ladder_at_capacity(2, 2));
        assert!(ladder_at_capacity(2, 5));
        assert!(ladder_at_capacity(0, 0));
        assert!(ladder_at_capacity(0, 1));
    }
}
