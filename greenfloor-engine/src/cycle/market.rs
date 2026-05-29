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

pub fn market_cycle_phases() -> &'static [MarketCyclePhase] {
    &[
        MarketCyclePhase::Reconcile,
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

pub fn needs_inventory_fallback(bucket_counts_available: bool, coinset_scan_empty: bool) -> bool {
    !bucket_counts_available || coinset_scan_empty
}

pub fn wallet_fallback_source_label(coinset_scan_empty: bool) -> &'static str {
    if coinset_scan_empty {
        "wallet_adapter_fallback_after_empty_coinset_scan"
    } else {
        "wallet_adapter"
    }
}

pub fn resolve_inventory_scan_source(
    coinset_scan_found_coins: bool,
    coinset_scan_empty: bool,
    cat_scan_found_coins: bool,
    wallet_scan_found_coins: bool,
) -> &'static str {
    if coinset_scan_found_coins {
        return "coinset";
    }
    if cat_scan_found_coins && coinset_scan_empty {
        return "coinset_cat_scan_fallback_after_empty_coinset_scan";
    }
    if wallet_scan_found_coins {
        return wallet_fallback_source_label(coinset_scan_empty);
    }
    "config_seed_or_no_asset_scan"
}

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

pub fn one_sided_offer_counts_by_side(
    sell_counts: &BTreeMap<i64, i64>,
    tracked_sizes: &[i64],
) -> (BTreeMap<i64, i64>, BTreeMap<i64, i64>) {
    let mut buy = BTreeMap::new();
    let mut sell = BTreeMap::new();
    for size in tracked_sizes {
        buy.insert(*size, 0);
        sell.insert(*size, sell_counts.get(size).copied().unwrap_or(0));
    }
    (buy, sell)
}

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
            resolve_inventory_scan_source(true, false, false, false),
            "coinset"
        );
    }

    #[test]
    fn resolve_inventory_scan_source_uses_cat_after_empty_coinset() {
        assert_eq!(
            resolve_inventory_scan_source(false, true, true, false),
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
}
