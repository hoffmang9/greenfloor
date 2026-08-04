//! Deterministic bootstrap mixed-output planner for offer denomination preflight — thin
//! wrapper over `coin_ops::shape` (base units, ladder-row protection always on).
//!
//! `output_amounts_base_units` is the authoritative mixed-split output list for
//! `run_signer_denomination_phase` (passed to vault mixed-split as `output_amounts`).

use crate::coin_ops::shape::{
    plan_shape_from_deficits, ShapeCoin, ShapeFunding, ShapeFundingOptions, ShapeLadderRow,
    ShapePlanOutcome,
};

use super::amounts::BaseUnits;
use super::combine_plan::BootstrapCombineContext;
use super::plan::{
    BootstrapCoin, BootstrapFundingSource, BootstrapPlan, BootstrapPlanOutcome, LadderDeficit,
    PlannerLadderRow,
};

fn validate_inputs(
    ladder_entries: &[PlannerLadderRow],
    spendable_coins: &[BootstrapCoin],
) -> Option<BootstrapPlanOutcome> {
    if ladder_entries.is_empty()
        || !ladder_entries.iter().all(|row| {
            row.size_base_units > 0 && row.target_count >= 0 && row.split_buffer_count >= 0
        })
    {
        return Some(BootstrapPlanOutcome::InvalidLadder);
    }
    if !spendable_coins.iter().all(|coin| coin.amount.get() >= 0) {
        return Some(BootstrapPlanOutcome::InvalidCoins);
    }
    None
}

/// Bootstrap ladder rows -> unit-agnostic shape rows. Shared with [`super::combine_plan`]'s
/// test-only combine builder so bootstrap has one row/coin conversion, not two.
pub(super) fn to_shape_rows(sorted_ladder: &[PlannerLadderRow]) -> Vec<ShapeLadderRow> {
    sorted_ladder
        .iter()
        .map(|row| ShapeLadderRow {
            size: row.size_base_units,
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

fn to_bootstrap_funding(funding: ShapeFunding) -> BootstrapFundingSource {
    match funding {
        ShapeFunding::SingleCoin { coin_id, amount } => BootstrapFundingSource::SingleCoin {
            coin_id,
            amount: BaseUnits::new(amount),
        },
        ShapeFunding::CombineFirst(inputs) => BootstrapFundingSource::CombineFirst(inputs),
    }
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
    sorted_ladder.sort_by_key(|row| row.size_base_units);

    let options = ShapeFundingOptions::bootstrap(
        combine_input_cap,
        &combine_context.canonical_asset_id,
        combine_context.mojo_multiplier,
    );

    match plan_shape_from_deficits(
        &to_shape_rows(&sorted_ladder),
        &to_shape_coins(spendable_coins),
        &options,
    ) {
        ShapePlanOutcome::Ready => BootstrapPlanOutcome::Ready,
        ShapePlanOutcome::NeedsShape(plan) => {
            BootstrapPlanOutcome::NeedsShape(BootstrapPlan::needs_shape(
                to_bootstrap_funding(plan.funding),
                plan.total_output_amount,
                plan.output_amounts,
                plan.deficits
                    .into_iter()
                    .map(|deficit| {
                        LadderDeficit::new(
                            deficit.size,
                            deficit.required_count,
                            deficit.current_count,
                        )
                    })
                    .collect(),
            ))
        }
        ShapePlanOutcome::CannotFund { required_amount } => BootstrapPlanOutcome::CannotFund {
            total_output_amount: required_amount,
        },
    }
}

#[cfg(test)]
mod tests;
