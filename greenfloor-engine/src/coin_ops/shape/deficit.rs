//! Ladder-row deficit collection shared by bootstrap and daemon shape planning.

use std::collections::HashMap;

use super::types::{ShapeDeficit, ShapeLadderRow};
use crate::coin_ops::shape_protection::{required_ladder_row_slots, LadderShapeContext};

fn required_rows(rows: &[ShapeLadderRow]) -> Vec<(i64, i64)> {
    required_ladder_row_slots(
        rows.iter()
            .map(|row| (row.size, row.target_count, row.split_buffer_count)),
    )
}

/// Shape context for a set of ladder rows and spendable amounts (same unit system).
#[must_use]
pub fn shape_context_for_rows(
    rows: &[ShapeLadderRow],
    spendable_amounts: &[i64],
) -> LadderShapeContext {
    LadderShapeContext::from_required_rows(&required_rows(rows), spendable_amounts)
}

/// Protected-slot map (`size -> target_count + split_buffer_count`) for `rows`, independent
/// of current inventory. Used by [`super::combine::plan_ladder_preserving_combine`].
#[must_use]
pub fn protected_slots_for_rows(rows: &[ShapeLadderRow]) -> HashMap<i64, i64> {
    required_rows(rows).into_iter().collect()
}

/// Deficits and mixed-split output amounts from shape context (`sorted_rows` sorted by size).
///
/// # Panics
///
/// Panics if a computed deficit somehow exceeds `i64::MAX` as a `usize` (unreachable for any
/// realistic ladder configuration).
#[must_use]
pub fn collect_shape_deficits(
    sorted_rows: &[ShapeLadderRow],
    shape_ctx: &LadderShapeContext,
) -> (Vec<ShapeDeficit>, Vec<i64>) {
    let mut deficits = Vec::new();
    let mut output_amounts = Vec::new();
    for row in sorted_rows {
        let size = row.size;
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
        deficits.push(ShapeDeficit {
            size,
            required_count: required,
            current_count: current,
        });
        output_amounts.extend(std::iter::repeat_n(
            size,
            usize::try_from(deficit).expect("deficit is positive"),
        ));
    }
    (deficits, output_amounts)
}
