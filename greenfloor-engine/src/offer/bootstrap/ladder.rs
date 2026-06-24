//! Shared ladder row invariants for bootstrap planning.

use crate::coin_ops::shape_protection::{
    required_ladder_row_slots, select_smallest_non_cannibalizing_candidate_id, LadderShapeContext,
    SplittableCandidate,
};

use super::planner::{BootstrapCoin, PlannerLadderRow};

/// Shape context for bootstrap planner / preflight from ladder rows and spendable amounts.
#[must_use]
pub(crate) fn ladder_shape_context_for_bootstrap(
    ladder_entries: &[PlannerLadderRow],
    spendable_amounts_base_units: &[i64],
) -> LadderShapeContext {
    LadderShapeContext::from_required_rows(
        &required_ladder_row_slots(ladder_entries.iter().map(|row| {
            (
                row.size_base_units,
                row.target_count,
                row.split_buffer_count,
            )
        })),
        spendable_amounts_base_units,
    )
}

/// Smallest bootstrap coin that can fund a split without cannibalizing a protected ladder row.
#[must_use]
pub(crate) fn select_smallest_non_cannibalizing_bootstrap_coin<'a>(
    spendable_coins: &'a [BootstrapCoin],
    required_output_base_units: i64,
    shape_ctx: &LadderShapeContext,
) -> Option<&'a BootstrapCoin> {
    let candidates: Vec<SplittableCandidate<'_>> = spendable_coins
        .iter()
        .filter(|coin| !coin.id.trim().is_empty())
        .map(|coin| SplittableCandidate {
            id: coin.id.as_str(),
            amount_base_units: coin.amount.get(),
        })
        .collect();
    let selected_id = select_smallest_non_cannibalizing_candidate_id(
        &candidates,
        required_output_base_units,
        shape_ctx,
    )?;
    spendable_coins.iter().find(|coin| coin.id == selected_id)
}
