//! Deterministic bootstrap mixed-output planner for offer denomination preflight — thin
//! wrapper over `coin_ops::shape` (ladder-row protection always on).
//!
//! Plan amounts and ladder sizes share one unit: mojos on the signer denomination path,
//! or config/plan units in planner fixtures (see [`BootstrapCombineContext`]).
//!
//! `output_amounts` is the authoritative mixed-split output list for
//! `run_signer_denomination_phase` (passed to vault mixed-split as `output_amounts`).

use crate::coin_ops::shape::{
    plan_shape_from_deficits, AmountUnit, ShapeCoin, ShapeFundingPolicy, ShapeLadderRow,
    ShapePlanOutcome,
};

use super::plan::{BootstrapCoin, BootstrapPlan, BootstrapPlanOutcome, PlannerLadderRow};

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

fn validate_inputs(
    ladder_entries: &[PlannerLadderRow],
    spendable_coins: &[BootstrapCoin],
) -> Option<BootstrapPlanOutcome> {
    if ladder_entries.is_empty()
        || !ladder_entries
            .iter()
            .all(|row| row.size > 0 && row.target_count >= 0 && row.split_buffer_count >= 0)
    {
        return Some(BootstrapPlanOutcome::InvalidLadder);
    }
    if !spendable_coins.iter().all(|coin| coin.amount.get() >= 0) {
        return Some(BootstrapPlanOutcome::InvalidCoins);
    }
    None
}

/// Bootstrap ladder rows -> unit-agnostic shape rows. Shared with [`super::shape_policy`]
/// (bootstrap ownership adapter) so bootstrap has one row/coin conversion.
pub(super) fn to_shape_rows(sorted_ladder: &[PlannerLadderRow]) -> Vec<ShapeLadderRow> {
    sorted_ladder
        .iter()
        .map(|row| ShapeLadderRow {
            size: row.size,
            target_count: row.target_count,
            split_buffer_count: row.split_buffer_count,
        })
        .collect()
}

pub(super) fn to_shape_coins(coins: &[BootstrapCoin]) -> Vec<ShapeCoin> {
    coins
        .iter()
        .map(|coin| ShapeCoin::new(coin.id.clone(), coin.amount.get()))
        .collect()
}

/// Build a one-shot mixed-output bootstrap plan from ladder deficits.
#[must_use]
pub fn plan_bootstrap_mixed_outputs(
    ladder_entries: &[PlannerLadderRow],
    spendable_coins: &[BootstrapCoin],
    combine_input_cap: i64,
    combine_context: &BootstrapCombineContext,
) -> BootstrapPlanOutcome {
    if let Some(outcome) = validate_inputs(ladder_entries, spendable_coins) {
        return outcome;
    }

    let mut sorted_ladder = ladder_entries.to_vec();
    sorted_ladder.sort_by_key(|row| row.size);

    let policy = ShapeFundingPolicy::Bootstrap {
        combine_input_cap,
        canonical_asset_id: &combine_context.canonical_asset_id,
        unit: combine_context.unit,
    };

    match plan_shape_from_deficits(
        &to_shape_rows(&sorted_ladder),
        &to_shape_coins(spendable_coins),
        &policy,
    ) {
        ShapePlanOutcome::Ready => BootstrapPlanOutcome::Ready,
        ShapePlanOutcome::NeedsShape(plan) => {
            BootstrapPlanOutcome::NeedsShape(BootstrapPlan::needs_shape(
                plan.funding,
                plan.total_output_amount,
                plan.output_amounts,
                plan.deficits,
            ))
        }
        ShapePlanOutcome::CannotFund { required_amount } => BootstrapPlanOutcome::CannotFund {
            total_output_amount: required_amount,
        },
    }
}

#[cfg(test)]
mod tests;
