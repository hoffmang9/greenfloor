use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarketCyclePhase {
    Reconcile,
    Inventory,
    Strategy,
    Cancel,
    CoinOps,
}

impl MarketCyclePhase {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reconcile => "reconcile",
            Self::Inventory => "inventory",
            Self::Strategy => "strategy",
            Self::Cancel => "cancel",
            Self::CoinOps => "coin_ops",
        }
    }
}

#[must_use]
pub fn market_cycle_phases() -> &'static [MarketCyclePhase] {
    &[
        MarketCyclePhase::Reconcile,
        MarketCyclePhase::Inventory,
        MarketCyclePhase::Strategy,
        MarketCyclePhase::Cancel,
        MarketCyclePhase::CoinOps,
    ]
}

/// Phases run in-process after reconcile completes (reconcile is handled separately).
#[must_use]
pub fn post_reconcile_market_cycle_phases() -> &'static [MarketCyclePhase] {
    &[
        MarketCyclePhase::Inventory,
        MarketCyclePhase::Strategy,
        MarketCyclePhase::Cancel,
        MarketCyclePhase::CoinOps,
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MarketCycleResultState {
    pub cycle_errors: i64,
    pub strategy_planned: i64,
    pub strategy_executed: i64,
    pub cancel_triggered: bool,
    pub cancel_planned: i64,
    pub cancel_executed: i64,
    pub immediate_requeue_requested: bool,
    pub immediate_requeue_signals: Vec<String>,
}

impl MarketCycleResultState {
    pub fn record_phase_error(&mut self) {
        self.cycle_errors += 1;
    }

    pub fn merge_strategy_execution(&mut self, planned: i64, executed: i64) {
        self.strategy_planned += planned.max(0);
        self.strategy_executed += executed.max(0);
    }

    pub fn merge_cancel_policy(&mut self, triggered: bool, planned: i64, executed: i64) {
        if triggered {
            self.cancel_triggered = true;
        }
        self.cancel_planned += planned.max(0);
        self.cancel_executed += executed.max(0);
    }

    pub fn request_immediate_requeue(&mut self, signal: Option<String>) {
        self.immediate_requeue_requested = true;
        if let Some(value) = signal {
            let clean = value.trim();
            if !clean.is_empty() {
                self.immediate_requeue_signals.push(clean.to_string());
            }
        }
    }
}

#[must_use]
pub fn needs_inventory_fallback(bucket_counts_available: bool, coinset_scan_empty: bool) -> bool {
    !bucket_counts_available || coinset_scan_empty
}

#[must_use]
pub fn wallet_fallback_source_label(coinset_scan_empty: bool) -> &'static str {
    if coinset_scan_empty {
        "wallet_adapter_fallback_after_empty_coinset_scan"
    } else {
        "wallet_adapter"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoinsetInventoryScanState {
    pub found_coins: bool,
    pub empty: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SupplementalInventoryScanState {
    pub cat_found_coins: bool,
    pub wallet_found_coins: bool,
}

#[must_use]
pub fn resolve_inventory_scan_source(
    coinset: CoinsetInventoryScanState,
    supplemental: SupplementalInventoryScanState,
) -> &'static str {
    if coinset.found_coins {
        return "coinset";
    }
    if supplemental.cat_found_coins && coinset.empty {
        return "coinset_cat_scan_fallback_after_empty_coinset_scan";
    }
    if supplemental.wallet_found_coins {
        return wallet_fallback_source_label(coinset.empty);
    }
    "config_seed_or_no_asset_scan"
}

#[must_use]
pub fn resolve_tracked_sizes(ladder_sizes: &[i64], strategy_default_sizes: &[i64]) -> Vec<i64> {
    let mut tracked: Vec<i64> = ladder_sizes
        .iter()
        .copied()
        .filter(|size| *size > 0)
        .collect();
    if tracked.is_empty() {
        tracked = strategy_default_sizes
            .iter()
            .copied()
            .filter(|size| *size > 0)
            .collect();
    }
    tracked.sort_unstable();
    tracked.dedup();
    tracked
}

#[must_use]
pub fn aggregate_two_sided_offer_counts(
    buy_counts: &BTreeMap<i64, i64>,
    sell_counts: &BTreeMap<i64, i64>,
    tracked_sizes: &[i64],
) -> BTreeMap<i64, i64> {
    tracked_sizes
        .iter()
        .map(|size| {
            let total = buy_counts.get(size).copied().unwrap_or(0)
                + sell_counts.get(size).copied().unwrap_or(0);
            (*size, total)
        })
        .collect()
}

#[must_use]
pub fn one_sided_offer_counts_by_side(
    sell_counts: &BTreeMap<i64, i64>,
    tracked_sizes: &[i64],
) -> (BTreeMap<i64, i64>, BTreeMap<i64, i64>) {
    let mut buy = BTreeMap::default();
    let mut sell = BTreeMap::default();
    for size in tracked_sizes {
        buy.insert(*size, 0);
        sell.insert(*size, sell_counts.get(size).copied().unwrap_or(0));
    }
    (buy, sell)
}

#[must_use]
pub fn is_two_sided_market_mode(market_mode: &str) -> bool {
    market_mode.trim().eq_ignore_ascii_case("two_sided")
}

pub fn filter_positive_repeat_actions<T, F>(actions: &[T], repeat_of: F) -> usize
where
    F: Fn(&T) -> i64,
{
    actions
        .iter()
        .filter(|action| repeat_of(action) > 0)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_inventory_scan_source_prefers_coinset() {
        assert_eq!(
            resolve_inventory_scan_source(
                CoinsetInventoryScanState {
                    found_coins: true,
                    empty: false,
                },
                SupplementalInventoryScanState {
                    cat_found_coins: false,
                    wallet_found_coins: false,
                },
            ),
            "coinset"
        );
    }

    #[test]
    fn resolve_inventory_scan_source_uses_cat_after_empty_coinset() {
        assert_eq!(
            resolve_inventory_scan_source(
                CoinsetInventoryScanState {
                    found_coins: false,
                    empty: true,
                },
                SupplementalInventoryScanState {
                    cat_found_coins: true,
                    wallet_found_coins: false,
                },
            ),
            "coinset_cat_scan_fallback_after_empty_coinset_scan"
        );
    }

    #[test]
    fn resolve_tracked_sizes_falls_back_to_strategy_defaults() {
        assert_eq!(resolve_tracked_sizes(&[], &[1, 10, 100]), vec![1, 10, 100]);
    }

    #[test]
    fn merge_cancel_policy_accumulates() {
        let mut state = MarketCycleResultState::default();
        state.merge_cancel_policy(true, 2, 1);
        state.merge_cancel_policy(false, 1, 0);
        assert!(state.cancel_triggered);
        assert_eq!(state.cancel_planned, 3);
        assert_eq!(state.cancel_executed, 1);
    }

    #[test]
    fn market_cycle_phase_labels_and_order() {
        assert_eq!(MarketCyclePhase::Reconcile.as_str(), "reconcile");
        assert_eq!(market_cycle_phases().len(), 5);
        assert_eq!(post_reconcile_market_cycle_phases().len(), 4);
        assert!(!post_reconcile_market_cycle_phases().contains(&MarketCyclePhase::Reconcile));
    }

    #[test]
    fn market_cycle_result_state_tracks_errors_and_requeue() {
        let mut state = MarketCycleResultState::default();
        state.record_phase_error();
        state.record_phase_error();
        state.request_immediate_requeue(Some("taker_fill".to_string()));
        state.request_immediate_requeue(Some("  ".to_string()));
        state.request_immediate_requeue(None);
        assert_eq!(state.cycle_errors, 2);
        assert!(state.immediate_requeue_requested);
        assert_eq!(
            state.immediate_requeue_signals,
            vec!["taker_fill".to_string()]
        );
    }

    #[test]
    fn needs_inventory_fallback_and_wallet_source_labels() {
        assert!(needs_inventory_fallback(false, false));
        assert!(needs_inventory_fallback(true, true));
        assert!(!needs_inventory_fallback(true, false));
        assert_eq!(wallet_fallback_source_label(false), "wallet_adapter");
        assert_eq!(
            wallet_fallback_source_label(true),
            "wallet_adapter_fallback_after_empty_coinset_scan"
        );
    }

    #[test]
    fn resolve_inventory_scan_source_uses_wallet_when_coinset_empty() {
        assert_eq!(
            resolve_inventory_scan_source(
                CoinsetInventoryScanState {
                    found_coins: false,
                    empty: true,
                },
                SupplementalInventoryScanState {
                    cat_found_coins: false,
                    wallet_found_coins: true,
                },
            ),
            "wallet_adapter_fallback_after_empty_coinset_scan"
        );
        assert_eq!(
            resolve_inventory_scan_source(
                CoinsetInventoryScanState {
                    found_coins: false,
                    empty: false,
                },
                SupplementalInventoryScanState {
                    cat_found_coins: false,
                    wallet_found_coins: false,
                },
            ),
            "config_seed_or_no_asset_scan"
        );
    }

    #[test]
    fn aggregate_and_one_sided_offer_counts() {
        let buy = BTreeMap::from([(1, 2), (10, 1)]);
        let sell = BTreeMap::from([(1, 1), (10, 0)]);
        let tracked = vec![1, 10, 100];
        let total = aggregate_two_sided_offer_counts(&buy, &sell, &tracked);
        assert_eq!(total.get(&1), Some(&3));
        assert_eq!(total.get(&10), Some(&1));
        assert_eq!(total.get(&100), Some(&0));

        let (buy_only, sell_only) = one_sided_offer_counts_by_side(&sell, &tracked);
        assert_eq!(buy_only.get(&1), Some(&0));
        assert_eq!(sell_only.get(&1), Some(&1));
    }

    #[test]
    fn is_two_sided_market_mode_is_case_insensitive() {
        assert!(is_two_sided_market_mode("two_sided"));
        assert!(is_two_sided_market_mode(" TWO_SIDED "));
        assert!(!is_two_sided_market_mode("one_sided"));
    }

    #[test]
    fn filter_positive_repeat_actions_counts_only_positive() {
        let actions = vec![(0_i64, 0), (1, 2), (2, -1)];
        assert_eq!(
            filter_positive_repeat_actions(&actions, |action| action.1),
            1
        );
    }
}
