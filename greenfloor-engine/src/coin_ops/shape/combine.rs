//! Combine-first input selection for coin-shape planning (unit-agnostic; see [`super::AmountUnit`]).

use std::collections::{HashMap, HashSet};

use super::types::{CombineInputs, ShapeCoin};
use crate::coin_ops::cat_overshoot_change_would_be_dust;
use crate::coin_ops::selection::{
    select_spendable_coins_for_target_amount_with_options, SpendableCoin,
    TargetAmountSelectionOptions,
};
use crate::metrics::metric_non_negative_usize;

/// Select combine inputs covering `target_amount` from all of `coins` (no ladder-row
/// exclusions). Shared core for the daemon flat combine-prereq and the bootstrap
/// ladder-preserving retry in [`plan_ladder_preserving_combine`].
#[must_use]
pub fn plan_combine_inputs_for_target(
    coins: &[ShapeCoin],
    target_amount: i64,
    combine_input_cap: i64,
) -> Option<CombineInputs> {
    plan_combine_inputs_for_target_in(coins, target_amount, combine_input_cap, None)
}

/// [`plan_combine_inputs_for_target`] restricted to `allowed_coin_ids` when provided.
#[must_use]
pub(crate) fn plan_combine_inputs_for_target_in(
    coins: &[ShapeCoin],
    target_amount: i64,
    combine_input_cap: i64,
    allowed_coin_ids: Option<&HashSet<String>>,
) -> Option<CombineInputs> {
    if target_amount <= 0 {
        return None;
    }

    let cap = metric_non_negative_usize(combine_input_cap);
    if cap < 2 {
        return None;
    }

    let spendable: Vec<SpendableCoin> = coins
        .iter()
        .filter(|coin| {
            allowed_coin_ids.is_none_or(|allowed| allowed.contains(&coin.id))
                && !coin.id.trim().is_empty()
                && coin.amount > 0
        })
        .map(|coin| SpendableCoin::new(coin.id.clone(), coin.amount))
        .collect();

    let (unconstrained_ids, unconstrained_total, unconstrained_exact) =
        select_spendable_coins_for_target_amount_with_options(
            &spendable,
            target_amount,
            TargetAmountSelectionOptions::default(),
        );
    let selected_count_before_cap = unconstrained_ids.len();
    if selected_count_before_cap < 2 {
        return None;
    }

    let cap_applied = selected_count_before_cap > cap;
    let (input_coin_ids, selected_total, exact_match) = if cap_applied {
        let (ids, total, exact) = select_spendable_coins_for_target_amount_with_options(
            &spendable,
            target_amount,
            TargetAmountSelectionOptions::combine_cap(cap),
        );
        if ids.is_empty() {
            return None;
        }
        (ids, total, exact)
    } else {
        (unconstrained_ids, unconstrained_total, unconstrained_exact)
    };

    if cap_applied {
        tracing::info!(
            combine_input_cap = cap,
            selected_count_before_cap,
            selected_count = input_coin_ids.len(),
            selected_total,
            required_amount = target_amount,
            exact_match,
            "combine prereq input selection hit combine input cap"
        );
    }

    Some(CombineInputs {
        input_coin_ids,
        selected_total,
        target_amount,
        exact_match,
        cap_applied,
        selected_count_before_cap,
        combine_input_cap,
    })
}

/// Partition `coins` into (eligible, excluded) by `protected_slots` (`size -> reserved count`).
///
/// Excluded coins are sorted smallest-amount-first so callers can release them one at a time
/// when an eligible-only selection is impossible.
fn partition_protected_coins(
    coins: &[ShapeCoin],
    protected_slots: &HashMap<i64, i64>,
) -> (Vec<ShapeCoin>, Vec<ShapeCoin>) {
    let mut protected_remaining = protected_slots.clone();
    let mut sorted = coins.to_vec();
    sorted.sort_by(|left, right| left.id.cmp(&right.id));

    let mut eligible = Vec::new();
    let mut excluded = Vec::new();
    for coin in sorted {
        if let Some(remaining) = protected_remaining.get_mut(&coin.amount) {
            if *remaining > 0 {
                *remaining -= 1;
                excluded.push(coin);
                continue;
            }
        }
        eligible.push(coin);
    }
    excluded.sort_by(|left, right| {
        left.amount
            .cmp(&right.amount)
            .then_with(|| left.id.cmp(&right.id))
    });
    (eligible, excluded)
}

fn combine_with_dust_guard(
    coins: &[ShapeCoin],
    target_amount: i64,
    combine_input_cap: i64,
    mojo_multiplier: i64,
    canonical_asset_id: &str,
    allowed_coin_ids: Option<&HashSet<String>>,
) -> Option<CombineInputs> {
    let selection = plan_combine_inputs_for_target_in(
        coins,
        target_amount,
        combine_input_cap,
        allowed_coin_ids,
    )?;
    let change = selection
        .selected_total
        .saturating_sub(selection.target_amount);
    let change_mojos = change.saturating_mul(mojo_multiplier.max(1));
    if cat_overshoot_change_would_be_dust(change_mojos, canonical_asset_id) {
        return None;
    }
    Some(selection)
}

/// Combine-first input selection with ladder-row preservation retry (bootstrap combine body).
///
/// Coins whose amount exactly matches a `protected_slots` size are excluded from the first
/// selection attempt. If no covering, non-dust selection is possible with only eligible
/// coins, protected coins are released smallest-amount-first before falling back to an
/// unconstrained selection over all of `coins`.
#[must_use]
pub fn plan_ladder_preserving_combine(
    coins: &[ShapeCoin],
    protected_slots: &HashMap<i64, i64>,
    target_amount: i64,
    combine_input_cap: i64,
    mojo_multiplier: i64,
    canonical_asset_id: &str,
) -> Option<CombineInputs> {
    let (eligible, excluded) = partition_protected_coins(coins, protected_slots);
    let mut allowed_ids: HashSet<String> = eligible.iter().map(|coin| coin.id.clone()).collect();

    if let Some(plan) = combine_with_dust_guard(
        coins,
        target_amount,
        combine_input_cap,
        mojo_multiplier,
        canonical_asset_id,
        Some(&allowed_ids),
    ) {
        return Some(plan);
    }

    for coin in excluded {
        allowed_ids.insert(coin.id.clone());
        if let Some(plan) = combine_with_dust_guard(
            coins,
            target_amount,
            combine_input_cap,
            mojo_multiplier,
            canonical_asset_id,
            Some(&allowed_ids),
        ) {
            return Some(plan);
        }
    }

    combine_with_dust_guard(
        coins,
        target_amount,
        combine_input_cap,
        mojo_multiplier,
        canonical_asset_id,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coins(rows: &[(&str, i64)]) -> Vec<ShapeCoin> {
        rows.iter()
            .map(|(id, amount)| ShapeCoin::new(*id, *amount))
            .collect()
    }

    #[test]
    fn partition_excludes_protected_amounts_up_to_reserved_count() {
        let protected: HashMap<i64, i64> = HashMap::from([(10, 2)]);
        let (eligible, excluded) = partition_protected_coins(
            &coins(&[("a", 10), ("b", 10), ("c", 10), ("d", 5)]),
            &protected,
        );
        assert_eq!(excluded.len(), 2);
        assert_eq!(eligible.len(), 2);
        assert!(eligible.iter().any(|coin| coin.id == "d"));
        assert!(eligible.iter().any(|coin| coin.amount == 10));
    }

    #[test]
    fn plan_combine_inputs_requires_at_least_two_inputs() {
        assert!(plan_combine_inputs_for_target(&coins(&[("only", 15_000)]), 10_000, 10).is_none());
    }

    #[test]
    fn ladder_preserving_combine_returns_none_when_total_inventory_is_insufficient() {
        let protected: HashMap<i64, i64> = HashMap::from([(10, 1)]);
        let all_coins = coins(&[("ten", 10), ("five", 5)]);
        assert!(plan_ladder_preserving_combine(&all_coins, &protected, 100, 5, 1, "xch").is_none());
    }

    #[test]
    fn ladder_preserving_combine_releases_protected_coin_when_needed() {
        let protected: HashMap<i64, i64> = HashMap::from([(10, 3)]);
        let all_coins = coins(&[("ten_a", 10), ("ten_b", 10), ("ten_c", 10), ("five", 5)]);
        let plan = plan_ladder_preserving_combine(&all_coins, &protected, 15, 5, 1, "xch")
            .expect("releases one protected ten to cover 15");
        assert!(plan.input_coin_ids.contains(&"five".to_string()));
        assert_eq!(plan.input_coin_ids.len(), 2);
    }
}
