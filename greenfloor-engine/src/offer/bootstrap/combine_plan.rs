//! Bootstrap combine-first input selection (base units only).

use super::amounts::{bootstrap_overshoot_change_mojos, BaseUnits};
use super::combine_inputs::BootstrapCombineInputs;
use super::planner::BootstrapCoin;
use crate::coin_ops::cat_overshoot_change_would_be_dust;
use crate::coin_ops::select_combine_inputs_for_target;
use crate::coin_ops::TargetAmountCoin;

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

/// Build combine-first inputs for bootstrap shaping (`BootstrapCoin` amounts are base units).
#[must_use]
pub fn build_bootstrap_combine_plan(
    coins: &[BootstrapCoin],
    target_amount_base_units: BaseUnits,
    combine_input_cap: i64,
    combine_context: &BootstrapCombineContext,
) -> Option<BootstrapCombineInputs> {
    let candidates = selection_candidates(coins);
    let selection = select_combine_inputs_for_target(
        &candidates,
        target_amount_base_units.get(),
        combine_input_cap,
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

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(build_bootstrap_combine_plan(&spendable, BaseUnits::new(100), 10, &ctx).is_none());
    }
}
