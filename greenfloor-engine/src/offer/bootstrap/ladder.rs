//! Shared ladder row invariants for bootstrap planning.

use std::collections::HashMap;

use crate::coin_ops::shape_protection::{required_ladder_row_slot, LadderShapeContext};

use super::planner::PlannerLadderRow;

fn required_rows(ladder_entries: &[PlannerLadderRow]) -> Vec<(i64, i64)> {
    ladder_entries
        .iter()
        .map(|row| {
            required_ladder_row_slot(
                row.size_base_units,
                row.target_count,
                row.split_buffer_count,
            )
        })
        .collect()
}

/// Required exact-size coin slots per ladder row (`target_count + split_buffer_count`).
#[must_use]
pub(crate) fn protected_ladder_coin_slots_by_size(
    ladder_entries: &[PlannerLadderRow],
) -> HashMap<i64, i64> {
    LadderShapeContext::from_required_rows(&required_rows(ladder_entries), &[]).protected_slots
}

/// Shape context for bootstrap planner / preflight from ladder rows and spendable amounts.
#[must_use]
pub(crate) fn ladder_shape_context_for_bootstrap(
    ladder_entries: &[PlannerLadderRow],
    spendable_amounts_base_units: &[i64],
) -> LadderShapeContext {
    LadderShapeContext::from_required_rows(
        &required_rows(ladder_entries),
        spendable_amounts_base_units,
    )
}
