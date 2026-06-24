//! Shared ladder row invariants for bootstrap planning.

use std::collections::HashMap;

use super::planner::PlannerLadderRow;

/// Required exact-size coin slots per ladder row (`target_count + split_buffer_count`).
#[must_use]
pub(crate) fn protected_ladder_coin_slots_by_size(
    ladder_entries: &[PlannerLadderRow],
) -> HashMap<i64, i64> {
    let mut required = HashMap::new();
    for row in ladder_entries {
        required.insert(
            row.size_base_units,
            row.target_count + row.split_buffer_count,
        );
    }
    required
}
