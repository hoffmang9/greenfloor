//! Bootstrap combine-first input selection (base units only) — thin wrapper over
//! `coin_ops::shape::plan_ladder_preserving_combine`.
//!
//! [`build_bootstrap_combine_plan`] is test-only: `plan_bootstrap_mixed_outputs` (via
//! `coin_ops::shape::plan_shape_from_deficits`) is the sole production door for bootstrap
//! combine-first funding. This module's regression tests exercise the shared
//! `plan_ladder_preserving_combine` core directly at the bootstrap unit boundary.

#[cfg(test)]
use super::plan::{BootstrapCoin, PlannerLadderRow};

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

#[cfg(test)]
fn build_bootstrap_combine_plan(
    coins: &[BootstrapCoin],
    ladder_entries: &[PlannerLadderRow],
    target_amount_base_units: super::amounts::BaseUnits,
    combine_input_cap: i64,
    combine_context: &BootstrapCombineContext,
) -> Option<crate::coin_ops::shape::CombineInputs> {
    use super::planner::{to_shape_coins, to_shape_rows};
    use crate::coin_ops::shape::{plan_ladder_preserving_combine, protected_slots_for_rows};

    let protected_slots = protected_slots_for_rows(&to_shape_rows(ladder_entries));
    plan_ladder_preserving_combine(
        &to_shape_coins(coins),
        &protected_slots,
        target_amount_base_units.get(),
        combine_input_cap,
        combine_context.mojo_multiplier,
        &combine_context.canonical_asset_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offer::bootstrap::amounts::BaseUnits;
    use crate::test_support::eco181_bootstrap_inventory::{
        eco181_bootstrap_coins, eco181_bootstrap_ladder,
    };
    use crate::test_support::fragmented_combine_cap_inventory::fragmented_combine_cap_spendable_coins;

    use crate::offer::bootstrap::test_fixtures::bootstrap_coin as coin;

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
        assert_eq!(inputs.selected_total, 105);
        assert!(!inputs.exact_match);
        assert_eq!(inputs.target_amount, 100);
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
        assert_eq!(inputs.target_amount, 100);
    }
}
