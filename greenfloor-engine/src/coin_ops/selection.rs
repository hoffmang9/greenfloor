//! Low-level coin selection helpers for split/combine planning.

use std::collections::HashSet;

use super::policy::coin_op_min_amount_mojos;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpendableCoin {
    pub id: String,
    pub amount: i64,
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

#[must_use]
pub fn split_would_create_sub_cat_change(
    selected_amount_mojos: i64,
    required_amount_mojos: i64,
    canonical_asset_id: &str,
) -> (bool, i64) {
    let remainder = selected_amount_mojos - required_amount_mojos;
    let min_cat_mojos = coin_op_min_amount_mojos(canonical_asset_id);
    if min_cat_mojos > 0 && remainder > 0 && remainder < min_cat_mojos {
        (true, remainder)
    } else {
        (false, remainder)
    }
}

#[must_use]
pub fn select_spendable_coins_for_target_amount(
    coins: &[SpendableCoin],
    target_amount: i64,
) -> (Vec<String>, i64, bool) {
    let required = target_amount;
    if required <= 0 {
        return (Vec::new(), 0, false);
    }
    let entries: Vec<(String, i64)> = coins
        .iter()
        .filter(|coin| !coin.id.is_empty() && coin.amount > 0)
        .map(|coin| (coin.id.clone(), coin.amount))
        .collect();
    if entries.is_empty() {
        return (Vec::new(), 0, false);
    }

    let max_amount = entries.iter().map(|(_, amount)| *amount).max().unwrap_or(0);
    let cap = required + max_amount;
    if cap > 500_000 {
        let mut ordered = entries.clone();
        ordered.sort_by_key(|(_, amount)| std::cmp::Reverse(*amount));
        let mut picked_ids = Vec::new();
        let mut running = 0i64;
        for (coin_id, amount) in ordered {
            picked_ids.push(coin_id);
            running += amount;
            if running >= required {
                return (picked_ids, running, running == required);
            }
        }
        return (Vec::new(), 0, false);
    }

    let mut best: std::collections::BTreeMap<i64, Vec<usize>> =
        std::collections::BTreeMap::default();
    best.insert(0, Vec::new());
    for (idx, (_, amount)) in entries.iter().enumerate() {
        let snapshot: Vec<(i64, Vec<usize>)> = best.iter().map(|(s, v)| (*s, v.clone())).collect();
        for (prev_sum, subset) in snapshot {
            let next_sum = prev_sum + amount;
            if next_sum > cap {
                continue;
            }
            let mut candidate = subset;
            candidate.push(idx);
            let existing = best.get(&next_sum);
            if existing.is_none_or(|e| candidate.len() < e.len()) {
                best.insert(next_sum, candidate);
            }
        }
    }

    if let Some(exact_subset) = best.get(&required) {
        if !exact_subset.is_empty() {
            let ids: Vec<String> = exact_subset.iter().map(|i| entries[*i].0.clone()).collect();
            let total: i64 = exact_subset.iter().map(|i| entries[*i].1).sum();
            return (ids, total, true);
        }
    }

    let overs: Vec<i64> = best.keys().copied().filter(|s| *s > required).collect();
    if overs.is_empty() {
        return (Vec::new(), 0, false);
    }
    let best_over = overs
        .into_iter()
        .min_by_key(|sum| {
            let subset = best.get(sum).map_or(0, Vec::len);
            (sum - required, subset, *sum)
        })
        .unwrap_or(0);
    let subset = best.get(&best_over).cloned().unwrap_or_default();
    if subset.is_empty() {
        return (Vec::new(), 0, false);
    }
    let ids: Vec<String> = subset.iter().map(|i| entries[*i].0.clone()).collect();
    let total: i64 = subset.iter().map(|i| entries[*i].1).sum();
    (ids, total, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coins(rows: &[(&str, i64)]) -> Vec<SpendableCoin> {
        rows.iter()
            .map(|(id, amount)| SpendableCoin {
                id: (*id).to_string(),
                amount: *amount,
            })
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
}
