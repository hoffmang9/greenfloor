//! Deterministic bootstrap mixed-output planner for offer denomination preflight.
//!
//! `output_amounts_base_units` is the authoritative mixed-split output list for
//! `run_signer_denomination_phase` (passed to vault mixed-split as `output_amounts`).

use crate::coin_ops::aggregate_covers_without_single_coin;
use crate::coin_ops::shape_protection::LadderShapeContext;

use super::amounts::BaseUnits;
use super::combine_plan::{build_bootstrap_combine_plan, BootstrapCombineContext};
use super::ladder::{
    ladder_shape_context_for_bootstrap, select_smallest_non_cannibalizing_bootstrap_coin,
};
use super::plan::{
    bootstrap_coin_amounts, spendable_bootstrap_coins, BootstrapCoin, BootstrapFundingSource,
    BootstrapPlan, BootstrapPlanOutcome, LadderDeficit, PlannerLadderRow,
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

fn collect_ladder_deficits(
    sorted_ladder: &[PlannerLadderRow],
    shape_ctx: &LadderShapeContext,
) -> (Vec<LadderDeficit>, Vec<i64>) {
    let mut deficits = Vec::new();
    let mut output_amounts = Vec::new();
    for row in sorted_ladder {
        let size = row.size_base_units;
        let required = shape_ctx.protected_slots.get(&size).copied().unwrap_or(0);
        let current = shape_ctx
            .exact_ladder_counts
            .get(&size)
            .copied()
            .unwrap_or(0);
        let deficit = required - current;
        if deficit <= 0 {
            continue;
        }
        deficits.push(LadderDeficit {
            size_base_units: size,
            required_count: required,
            current_count: current,
            deficit_count: deficit,
        });
        output_amounts.extend(std::iter::repeat_n(
            size,
            usize::try_from(deficit).expect("deficit is positive"),
        ));
    }
    (deficits, output_amounts)
}

struct FundedShapePlanInput<'a> {
    spendable_coins: &'a [BootstrapCoin],
    spendable_amounts: &'a [i64],
    sorted_ladder: &'a [PlannerLadderRow],
    total_output_amount: i64,
    combine_input_cap: i64,
    combine_context: &'a BootstrapCombineContext,
    shape_ctx: &'a LadderShapeContext,
    output_amounts: Vec<i64>,
    deficits: Vec<LadderDeficit>,
}

fn build_funded_shape_plan(input: FundedShapePlanInput<'_>) -> BootstrapPlanOutcome {
    let FundedShapePlanInput {
        spendable_coins,
        spendable_amounts,
        sorted_ladder,
        total_output_amount,
        combine_input_cap,
        combine_context,
        shape_ctx,
        output_amounts,
        deficits,
    } = input;
    if let Some(coin) = select_smallest_non_cannibalizing_bootstrap_coin(
        spendable_coins,
        total_output_amount,
        shape_ctx,
    ) {
        return BootstrapPlanOutcome::NeedsShape(BootstrapPlan::needs_shape(
            BootstrapFundingSource::SingleCoin {
                coin_id: coin.id.clone(),
                amount: coin.amount,
            },
            output_amounts,
            deficits,
        ));
    }
    if !aggregate_covers_without_single_coin(total_output_amount, spendable_amounts) {
        return BootstrapPlanOutcome::CannotFund {
            total_output_amount,
        };
    }
    let Some(combine_inputs) = build_bootstrap_combine_plan(
        &spendable_bootstrap_coins(spendable_coins),
        sorted_ladder,
        BaseUnits::new(total_output_amount),
        combine_input_cap,
        combine_context,
    ) else {
        return BootstrapPlanOutcome::CannotFund {
            total_output_amount,
        };
    };
    BootstrapPlanOutcome::NeedsShape(BootstrapPlan::needs_shape(
        BootstrapFundingSource::CombineFirst(combine_inputs),
        output_amounts,
        deficits,
    ))
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

    let spendable_amounts = bootstrap_coin_amounts(spendable_coins);
    let shape_ctx = ladder_shape_context_for_bootstrap(&sorted_ladder, &spendable_amounts);
    let (deficits, output_amounts) = collect_ladder_deficits(&sorted_ladder, &shape_ctx);

    if deficits.is_empty() {
        return BootstrapPlanOutcome::Ready;
    }

    let total_output_amount: i64 = output_amounts.iter().sum();
    build_funded_shape_plan(FundedShapePlanInput {
        spendable_coins,
        spendable_amounts: &spendable_amounts,
        sorted_ladder: &sorted_ladder,
        total_output_amount,
        combine_input_cap,
        combine_context,
        shape_ctx: &shape_ctx,
        output_amounts,
        deficits,
    })
}

#[cfg(test)]
mod tests;
