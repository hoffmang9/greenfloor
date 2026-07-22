//! Low-level coin selection helpers for split/combine planning.

use std::collections::HashSet;

use super::policy::cat_overshoot_change_would_be_dust;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetAmountOvershootRank {
    MinOvershoot,
    MinInputCount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TargetAmountSelectionOptions {
    pub max_input_count: Option<usize>,
    pub min_input_count: usize,
    pub overshoot_rank: TargetAmountOvershootRank,
}

impl Default for TargetAmountSelectionOptions {
    fn default() -> Self {
        Self {
            max_input_count: None,
            min_input_count: 1,
            overshoot_rank: TargetAmountOvershootRank::MinOvershoot,
        }
    }
}

impl TargetAmountSelectionOptions {
    pub(crate) fn combine_cap(cap: usize) -> Self {
        Self {
            max_input_count: Some(cap),
            min_input_count: 2,
            overshoot_rank: TargetAmountOvershootRank::MinInputCount,
        }
    }
}

/// Wallet coin for daemon coin-op selection (`amount` is always on-chain mojos).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpendableCoin {
    pub id: String,
    pub amount: i64,
    /// On-chain puzzle hash (64 hex). Empty in fixtures that only exercise amount selection.
    pub puzzle_hash: String,
}

impl SpendableCoin {
    #[must_use]
    pub fn new(id: impl Into<String>, amount: i64) -> Self {
        Self {
            id: id.into(),
            amount,
            puzzle_hash: String::new(),
        }
    }

    #[must_use]
    pub fn with_puzzle_hash(
        id: impl Into<String>,
        amount: i64,
        puzzle_hash: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            amount,
            puzzle_hash: puzzle_hash.into(),
        }
    }
}

#[must_use]
pub fn select_largest_spendable_coin<'a>(
    coins: &'a [SpendableCoin],
    min_amount_mojos: i64,
    exclude_coin_ids: &HashSet<String>,
) -> Option<&'a SpendableCoin> {
    coins
        .iter()
        .filter(|coin| {
            !coin.id.is_empty()
                && !exclude_coin_ids.contains(&coin.id)
                && coin.amount >= min_amount_mojos
        })
        .max_by_key(|coin| coin.amount)
}

#[must_use]
pub fn select_exact_amount_coin_ids(
    coins: &[SpendableCoin],
    amount_mojos: i64,
    exclude_coin_ids: &HashSet<String>,
    max_count: Option<usize>,
) -> Vec<String> {
    let mut selected = Vec::new();
    for coin in coins {
        if coin.id.is_empty() {
            continue;
        }
        if exclude_coin_ids.contains(&coin.id.to_ascii_lowercase()) {
            continue;
        }
        if coin.amount != amount_mojos {
            continue;
        }
        selected.push(coin.id.clone());
        if let Some(cap) = max_count {
            if selected.len() >= cap {
                break;
            }
        }
    }
    selected
}

/// Whether splitting `selected_amount_mojos` down to `required_amount_mojos` leaves CAT dust.
///
/// Both amounts must be in on-chain **mojos** (daemon coin-op paths only).
#[must_use]
pub fn split_would_create_sub_cat_change(
    selected_amount_mojos: i64,
    required_amount_mojos: i64,
    canonical_asset_id: &str,
) -> (bool, i64) {
    let remainder = selected_amount_mojos - required_amount_mojos;
    (
        cat_overshoot_change_would_be_dust(remainder, canonical_asset_id),
        remainder,
    )
}

#[must_use]
pub fn select_spendable_coins_for_target_amount(
    coins: &[SpendableCoin],
    target_amount: i64,
) -> (Vec<String>, i64, bool) {
    select_spendable_coins_for_target_amount_with_options(
        coins,
        target_amount,
        TargetAmountSelectionOptions::default(),
    )
}

#[must_use]
pub(crate) fn select_spendable_coins_for_target_amount_with_options(
    coins: &[SpendableCoin],
    target_amount: i64,
    options: TargetAmountSelectionOptions,
) -> (Vec<String>, i64, bool) {
    let required = target_amount;
    if required <= 0 {
        return (Vec::new(), 0, false);
    }

    let TargetAmountSelectionOptions {
        max_input_count,
        min_input_count,
        overshoot_rank,
    } = options;
    if min_input_count == 0 || max_input_count.is_some_and(|max| max < min_input_count) {
        return (Vec::new(), 0, false);
    }

    let entries = positive_spendable_entries(coins);
    if entries.len() < min_input_count {
        return (Vec::new(), 0, false);
    }

    let sum_cap = target_amount_sum_cap(required, &entries, max_input_count);
    if max_input_count.is_none() && sum_cap > 500_000 {
        return greedy_target_amount_selection(&entries, required, min_input_count);
    }

    let best = build_min_cardinality_subset_map(&entries, sum_cap, max_input_count);
    if let Some(exact) =
        exact_target_amount_subset(&best, &entries, required, min_input_count, max_input_count)
    {
        return exact;
    }

    choose_best_overshoot_subset(
        &best,
        &entries,
        required,
        min_input_count,
        max_input_count,
        overshoot_rank,
    )
}

fn positive_spendable_entries(coins: &[SpendableCoin]) -> Vec<(String, i64)> {
    coins
        .iter()
        .filter(|coin| !coin.id.is_empty() && coin.amount > 0)
        .map(|coin| (coin.id.clone(), coin.amount))
        .collect()
}

fn target_amount_sum_cap(
    required: i64,
    entries: &[(String, i64)],
    max_input_count: Option<usize>,
) -> i64 {
    let max_amount = entries.iter().map(|(_, amount)| *amount).max().unwrap_or(0);
    match max_input_count {
        Some(max) => required
            .saturating_add(max_amount.saturating_mul(i64::try_from(max).unwrap_or(i64::MAX))),
        None => required + max_amount,
    }
}

fn greedy_target_amount_selection(
    entries: &[(String, i64)],
    required: i64,
    min_input_count: usize,
) -> (Vec<String>, i64, bool) {
    let mut ordered = entries.to_vec();
    ordered.sort_by_key(|(_, amount)| std::cmp::Reverse(*amount));
    let mut picked_ids = Vec::new();
    let mut running = 0i64;
    for (coin_id, amount) in ordered {
        picked_ids.push(coin_id);
        running += amount;
        if running >= required && picked_ids.len() >= min_input_count {
            return (picked_ids, running, running == required);
        }
    }
    (Vec::new(), 0, false)
}

fn build_min_cardinality_subset_map(
    entries: &[(String, i64)],
    sum_cap: i64,
    max_input_count: Option<usize>,
) -> std::collections::BTreeMap<i64, Vec<usize>> {
    let max_subset_len = max_input_count.unwrap_or(usize::MAX);
    let mut best: std::collections::BTreeMap<i64, Vec<usize>> =
        std::collections::BTreeMap::default();
    best.insert(0, Vec::new());
    for (idx, (_, amount)) in entries.iter().enumerate() {
        let snapshot: Vec<(i64, Vec<usize>)> = best.iter().map(|(s, v)| (*s, v.clone())).collect();
        for (prev_sum, subset) in snapshot {
            if subset.len() >= max_subset_len {
                continue;
            }
            let next_sum = prev_sum + amount;
            if next_sum > sum_cap {
                continue;
            }
            let mut candidate = subset;
            candidate.push(idx);
            if best
                .get(&next_sum)
                .is_none_or(|existing| candidate.len() < existing.len())
            {
                best.insert(next_sum, candidate);
            }
        }
    }
    best
}

fn exact_target_amount_subset(
    best: &std::collections::BTreeMap<i64, Vec<usize>>,
    entries: &[(String, i64)],
    required: i64,
    min_input_count: usize,
    max_input_count: Option<usize>,
) -> Option<(Vec<String>, i64, bool)> {
    let exact_subset = best.get(&required)?;
    if exact_subset.len() < min_input_count {
        return None;
    }
    if max_input_count.is_some_and(|max| exact_subset.len() > max) {
        return None;
    }
    let ids: Vec<String> = exact_subset.iter().map(|i| entries[*i].0.clone()).collect();
    let total: i64 = exact_subset.iter().map(|i| entries[*i].1).sum();
    Some((ids, total, true))
}

fn choose_best_overshoot_subset(
    best: &std::collections::BTreeMap<i64, Vec<usize>>,
    entries: &[(String, i64)],
    required: i64,
    min_input_count: usize,
    max_input_count: Option<usize>,
    overshoot_rank: TargetAmountOvershootRank,
) -> (Vec<String>, i64, bool) {
    let mut chosen: Option<(i64, Vec<usize>)> = None;
    for (sum, subset) in best {
        if *sum < required || subset.len() < min_input_count {
            continue;
        }
        if max_input_count.is_some_and(|max| subset.len() > max) {
            continue;
        }
        if chosen.as_ref().is_none_or(|(best_sum, best_subset)| {
            overshoot_subset_better(
                *sum,
                subset,
                *best_sum,
                best_subset,
                required,
                overshoot_rank,
            )
        }) {
            chosen = Some((*sum, subset.clone()));
        }
    }
    let Some((sum, subset)) = chosen else {
        return (Vec::new(), 0, false);
    };
    let ids: Vec<String> = subset.iter().map(|i| entries[*i].0.clone()).collect();
    (ids, sum, sum == required)
}

fn overshoot_subset_better(
    candidate_sum: i64,
    candidate_subset: &[usize],
    best_sum: i64,
    best_subset: &[usize],
    required: i64,
    overshoot_rank: TargetAmountOvershootRank,
) -> bool {
    match overshoot_rank {
        TargetAmountOvershootRank::MinInputCount => {
            (
                candidate_subset.len(),
                candidate_sum - required,
                candidate_sum,
            ) < (best_subset.len(), best_sum - required, best_sum)
        }
        TargetAmountOvershootRank::MinOvershoot => {
            (
                candidate_sum - required,
                candidate_subset.len(),
                candidate_sum,
            ) < (best_sum - required, best_subset.len(), best_sum)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coins(rows: &[(&str, i64)]) -> Vec<SpendableCoin> {
        rows.iter()
            .map(|(id, amount)| SpendableCoin::new((*id).to_string(), *amount))
            .collect()
    }

    #[test]
    fn select_largest_respects_min_and_exclude() {
        let list = coins(&[("small", 100), ("big", 1500), ("excluded", 2000)]);
        let mut excluded: HashSet<String> = HashSet::default();
        excluded.insert("excluded".to_string());
        let picked = select_largest_spendable_coin(&list, 1000, &excluded);
        assert_eq!(picked.map(|c| c.id.as_str()), Some("big"));
    }

    #[test]
    fn select_exact_amount_case_insensitive_exclude() {
        let list = coins(&[("CoinA", 1000), ("coinb", 1000), ("CoinC", 2000)]);
        let mut excluded: HashSet<String> = HashSet::default();
        excluded.insert("coina".to_string());
        let ids = select_exact_amount_coin_ids(&list, 1000, &excluded, Some(5));
        assert_eq!(ids, vec!["coinb"]);
    }

    #[test]
    fn sub_cat_change_detects_dust_remainder() {
        let cat_id = "0000000000000000000000000000000000000000000000000000000000000001";
        let (dust, remainder) = split_would_create_sub_cat_change(10_500, 10_000, cat_id);
        assert!(dust);
        assert_eq!(remainder, 500);
    }

    #[test]
    fn sub_cat_change_allows_xch_remainder() {
        let (dust, remainder) = split_would_create_sub_cat_change(10_500, 10_000, "xch");
        assert!(!dust);
        assert_eq!(remainder, 500);
    }

    #[test]
    fn target_amount_prefers_exact_subset() {
        let list = coins(&[("c5", 5000), ("c3", 3000), ("c2", 2000), ("c3b", 3000)]);
        let (ids, total, exact) = select_spendable_coins_for_target_amount(&list, 10_000);
        assert!(exact);
        assert_eq!(total, 10_000);
        assert_eq!(ids.len(), 3);
        let set: HashSet<_> = ids.into_iter().collect();
        assert_eq!(set, HashSet::from(["c5", "c3", "c2"].map(str::to_string)));
    }

    #[test]
    fn target_amount_uses_change_when_no_exact() {
        let list = coins(&[("c5", 5000), ("c3a", 3000), ("c3b", 3000)]);
        let (ids, total, exact) = select_spendable_coins_for_target_amount(&list, 10_000);
        assert!(!exact);
        assert_eq!(total, 11_000);
        let set: HashSet<_> = ids.into_iter().collect();
        assert_eq!(set, HashSet::from(["c5", "c3a", "c3b"].map(str::to_string)));
    }

    #[test]
    fn target_amount_within_cap_prefers_minimum_cardinality_overshoot() {
        let list = coins(&[
            ("sixtyfive", 65),
            ("twenty", 20),
            ("ten_a", 10),
            ("ten_b", 10),
            ("ten_c", 10),
            ("three_a", 3),
            ("three_b", 3),
            ("three_c", 3),
            ("one_a", 1),
            ("one_b", 1),
        ]);
        let (ids, total, exact) = select_spendable_coins_for_target_amount_with_options(
            &list,
            100,
            TargetAmountSelectionOptions::combine_cap(5),
        );
        assert!(!exact);
        assert_eq!(total, 105);
        assert_eq!(ids.len(), 4);
        let set: HashSet<_> = ids.into_iter().collect();
        assert_eq!(
            set,
            HashSet::from(["sixtyfive", "twenty", "ten_a", "ten_b"].map(str::to_string))
        );
    }
}
