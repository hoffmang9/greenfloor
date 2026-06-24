//! Bootstrap combine-first input selection (base units only).

use std::collections::HashSet;

use super::amounts::{bootstrap_overshoot_change_mojos, BaseUnits};
use super::combine_inputs::BootstrapCombineInputs;
use super::ladder::protected_ladder_coin_slots_by_size;
use super::planner::{BootstrapCoin, PlannerLadderRow};
use crate::coin_ops::cat_overshoot_change_would_be_dust;
use crate::coin_ops::{select_combine_inputs_for_target_in, TargetAmountCoin};

/// Asset context for bootstrap combine dust validation at plan time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapCombineContext {
    pub mojo_multiplier: i64,
    pub canonical_asset_id: String,
}

impl BootstrapCombineContext {
    #[must_use]
    pub fn new(mojo_multiplier: i64, canonical_asset_id: impl Into<String>) -> Self {
        Self {
            mojo_multiplier,
            canonical_asset_id: canonical_asset_id.into(),
        }
    }

    #[must_use]
    pub fn for_tests() -> Self {
        Self::new(1_000, "xch")
    }
}

fn partition_ladder_coins(
    coins: &[BootstrapCoin],
    ladder_entries: &[PlannerLadderRow],
) -> (Vec<BootstrapCoin>, Vec<BootstrapCoin>) {
    let mut protected_remaining = protected_ladder_coin_slots_by_size(ladder_entries);
    let mut sorted = coins.to_vec();
    sorted.sort_by(|left, right| left.id.cmp(&right.id));

    let mut eligible = Vec::new();
    let mut excluded = Vec::new();
    for coin in sorted {
        let amount = coin.amount.get();
        if let Some(remaining) = protected_remaining.get_mut(&amount) {
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
            .get()
            .cmp(&right.amount.get())
            .then_with(|| left.id.cmp(&right.id))
    });
    (eligible, excluded)
}

fn selection_candidates(coins: &[BootstrapCoin]) -> Vec<TargetAmountCoin> {
    coins
        .iter()
        .filter(|coin| !coin.id.trim().is_empty() && coin.amount.get() > 0)
        .map(|coin| TargetAmountCoin {
            id: coin.id.clone(),
            amount: coin.amount.get(),
        })
        .collect()
}

fn build_bootstrap_combine_plan_in(
    coins: &[BootstrapCoin],
    target_amount_base_units: BaseUnits,
    combine_input_cap: i64,
    combine_context: &BootstrapCombineContext,
    allowed_coin_ids: Option<&HashSet<String>>,
) -> Option<BootstrapCombineInputs> {
    let candidates = selection_candidates(coins);
    let selection = select_combine_inputs_for_target_in(
        &candidates,
        target_amount_base_units.get(),
        combine_input_cap,
        allowed_coin_ids,
    )?;
    let selected_total = BaseUnits::new(selection.selected_total);
    let target_amount = BaseUnits::new(selection.target);
    let change_mojos = bootstrap_overshoot_change_mojos(
        selected_total,
        target_amount,
        combine_context.mojo_multiplier,
    );
    if cat_overshoot_change_would_be_dust(change_mojos, &combine_context.canonical_asset_id) {
        return None;
    }
    Some(BootstrapCombineInputs {
        input_coin_ids: selection.input_coin_ids,
        selected_total,
        target_amount,
        exact_match: selection.exact_match,
        cap_applied: selection.cap_applied,
    })
}

/// Build combine-first inputs for bootstrap shaping (`BootstrapCoin` amounts are base units).
///
/// When `ladder_entries` is non-empty, eligible inputs exclude coins reserved for exact ladder
/// sizes until a preserving selection is impossible.
#[must_use]
pub fn build_bootstrap_combine_plan(
    coins: &[BootstrapCoin],
    ladder_entries: &[PlannerLadderRow],
    target_amount_base_units: BaseUnits,
    combine_input_cap: i64,
    combine_context: &BootstrapCombineContext,
) -> Option<BootstrapCombineInputs> {
    let (eligible, excluded) = partition_ladder_coins(coins, ladder_entries);
    let mut allowed_ids: HashSet<String> = eligible.iter().map(|coin| coin.id.clone()).collect();

    if let Some(plan) = build_bootstrap_combine_plan_in(
        coins,
        target_amount_base_units,
        combine_input_cap,
        combine_context,
        Some(&allowed_ids),
    ) {
        return Some(plan);
    }

    for coin in excluded {
        allowed_ids.insert(coin.id.clone());
        if let Some(plan) = build_bootstrap_combine_plan_in(
            coins,
            target_amount_base_units,
            combine_input_cap,
            combine_context,
            Some(&allowed_ids),
        ) {
            return Some(plan);
        }
    }

    build_bootstrap_combine_plan_in(
        coins,
        target_amount_base_units,
        combine_input_cap,
        combine_context,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::eco181_bootstrap_inventory::{
        eco181_bootstrap_coins, eco181_bootstrap_ladder,
    };
    use crate::test_support::fragmented_combine_cap_inventory::fragmented_combine_cap_spendable_coins;

    fn coin(id: &str, amount: i64) -> BootstrapCoin {
        BootstrapCoin {
            id: id.to_string(),
            amount: BaseUnits::new(amount),
        }
    }

    const CAT_ASSET: &str = "0000000000000000000000000000000000000000000000000000000000000001";

    fn cat_combine_context() -> BootstrapCombineContext {
        BootstrapCombineContext::new(1_000, CAT_ASSET)
    }

    #[test]
    fn fragmented_inventory_within_cap_five() {
        let spendable: Vec<BootstrapCoin> = fragmented_combine_cap_spendable_coins()
            .into_iter()
            .map(|row| coin(&row.id, row.amount))
            .collect();
        let inputs = build_bootstrap_combine_plan(
            &spendable,
            &[],
            BaseUnits::new(100),
            5,
            &cat_combine_context(),
        )
        .expect("fragmented inventory should combine within cap=5");
        assert!(inputs.cap_applied);
        assert_eq!(inputs.input_coin_ids.len(), 4);
        assert_eq!(inputs.selected_total, BaseUnits::new(105));
        assert!(!inputs.exact_match);
        assert_eq!(inputs.target_amount, BaseUnits::new(100));
    }

    #[test]
    fn rejects_combine_when_overshoot_change_would_be_cat_dust() {
        let ctx = BootstrapCombineContext::new(1, CAT_ASSET);
        let spendable = vec![coin("a", 51), coin("b", 50)];
        assert!(
            build_bootstrap_combine_plan(&spendable, &[], BaseUnits::new(100), 10, &ctx).is_none()
        );
    }

    #[test]
    fn partition_protects_ladder_exact_inventory() {
        let ladder = eco181_bootstrap_ladder();
        let spendable = vec![
            coin("one_0", 1),
            coin("one_1", 1),
            coin("one_2", 1),
            coin("one_3", 1),
            coin("one_4", 1),
            coin("one_5", 1),
            coin("ten_0", 10),
            coin("ten_1", 10),
            coin("ten_2", 10),
            coin("five", 5),
            coin("eighty", 80),
        ];
        let (eligible, excluded) = partition_ladder_coins(&spendable, &ladder);
        let eligible_amounts: Vec<i64> = eligible.iter().map(|coin| coin.amount.get()).collect();
        assert!(!eligible_amounts.contains(&10));
        assert!(!eligible_amounts.contains(&1));
        assert!(eligible_amounts.contains(&80));
        assert!(eligible_amounts.contains(&5));
        assert_eq!(excluded.len(), 9);
    }

    #[test]
    fn preserving_ladder_combine_minimizes_ten_bu_inputs_for_eco181() {
        let inputs = build_bootstrap_combine_plan(
            &eco181_bootstrap_coins(),
            &eco181_bootstrap_ladder(),
            BaseUnits::new(100),
            5,
            &cat_combine_context(),
        )
        .expect("eco181 inventory should combine");
        assert!(
            inputs
                .input_coin_ids
                .iter()
                .filter(|id| id.starts_with("ten_"))
                .count()
                <= 1
        );
        assert_eq!(inputs.target_amount, BaseUnits::new(100));
    }
}
