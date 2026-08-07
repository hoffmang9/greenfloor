//! Bootstrap combine-first input selection — thin wrapper over
//! `coin_ops::shape::plan_ladder_preserving_combine`.
//!
//! [`build_bootstrap_combine_plan`] is test-only: `plan_bootstrap_mixed_outputs` (via
//! `coin_ops::shape::plan_shape_from_deficits`) is the sole production door for bootstrap
//! combine-first funding. This module's regression tests exercise the shared
//! `plan_ladder_preserving_combine` core directly at the bootstrap unit boundary.

use crate::coin_ops::shape::AmountUnit;

#[cfg(test)]
use super::plan::{BootstrapCoin, PlannerLadderRow};

/// Asset + amount-unit context for bootstrap combine dust validation at plan time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapCombineContext {
    pub unit: AmountUnit,
    pub canonical_asset_id: String,
}

impl BootstrapCombineContext {
    /// Denomination path: ladder and coins are already mojos.
    #[must_use]
    pub fn mojos(canonical_asset_id: impl Into<String>) -> Self {
        Self {
            unit: AmountUnit::Mojos {
                base_unit_mojo_multiplier: 1,
            },
            canonical_asset_id: canonical_asset_id.into(),
        }
    }

    /// Planner fixtures that still plan in config/plan units with a dust scale to mojos.
    #[must_use]
    pub fn plan_units(dust_mojo_multiplier: i64, canonical_asset_id: impl Into<String>) -> Self {
        Self {
            unit: AmountUnit::PlanUnits {
                dust_mojo_multiplier,
            },
            canonical_asset_id: canonical_asset_id.into(),
        }
    }

    #[must_use]
    pub fn for_tests() -> Self {
        Self::plan_units(1_000, "xch")
    }
}

#[cfg(test)]
fn build_bootstrap_combine_plan(
    coins: &[BootstrapCoin],
    ladder_entries: &[PlannerLadderRow],
    target_amount: super::amounts::PlanAmount,
    combine_input_cap: i64,
    combine_context: &BootstrapCombineContext,
) -> Option<crate::coin_ops::shape::CombineInputs> {
    use super::planner::{to_shape_coins, to_shape_rows};
    use crate::coin_ops::shape::{plan_ladder_preserving_combine, protected_slots_for_rows};

    let protected_slots = protected_slots_for_rows(&to_shape_rows(ladder_entries));
    plan_ladder_preserving_combine(
        &to_shape_coins(coins),
        &protected_slots,
        target_amount.get(),
        combine_input_cap,
        combine_context.unit.dust_change_mojo_multiplier(),
        &combine_context.canonical_asset_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offer::bootstrap::amounts::PlanAmount;
    use crate::test_support::eco181_bootstrap_inventory::{
        eco181_bootstrap_coins, eco181_bootstrap_ladder,
    };
    use crate::test_support::fragmented_combine_cap_inventory::fragmented_combine_cap_spendable_coins;

    use crate::offer::bootstrap::test_fixtures::{bootstrap_coin as coin, TEST_CAT_ASSET_ID};

    fn cat_combine_context() -> BootstrapCombineContext {
        BootstrapCombineContext::plan_units(1_000, TEST_CAT_ASSET_ID)
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
            PlanAmount::new(100),
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
        let ctx = BootstrapCombineContext::mojos(TEST_CAT_ASSET_ID);
        let spendable = vec![coin("a", 51), coin("b", 50)];
        assert!(
            build_bootstrap_combine_plan(&spendable, &[], PlanAmount::new(100), 10, &ctx).is_none()
        );
    }

    #[test]
    fn preserving_ladder_combine_minimizes_ten_bu_inputs_for_eco181() {
        let inputs = build_bootstrap_combine_plan(
            &eco181_bootstrap_coins(),
            &eco181_bootstrap_ladder(),
            PlanAmount::new(100),
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
