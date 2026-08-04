//! Shared ladder row invariants for bootstrap planning.

use crate::coin_ops::shape::shape_context_for_rows;
use crate::coin_ops::shape_protection::LadderShapeContext;

use super::plan::PlannerLadderRow;
use super::planner::to_shape_rows;

/// Shape context for bootstrap planner / preflight from ladder rows and spendable amounts.
#[must_use]
pub(crate) fn ladder_shape_context_for_bootstrap(
    ladder_entries: &[PlannerLadderRow],
    spendable_amounts_base_units: &[i64],
) -> LadderShapeContext {
    shape_context_for_rows(&to_shape_rows(ladder_entries), spendable_amounts_base_units)
}
