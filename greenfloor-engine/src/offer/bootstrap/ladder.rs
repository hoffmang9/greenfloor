//! Shared ladder row invariants for bootstrap planning.

use crate::coin_ops::shape_protection::{required_ladder_row_slots, LadderShapeContext};

use super::plan::PlannerLadderRow;

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
