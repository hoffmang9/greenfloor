//! Shared ladder row invariants for bootstrap planning.

use crate::coin_ops::shape_protection::{
    required_ladder_row_slots, select_smallest_non_cannibalizing_candidate_id, LadderShapeContext,
    SplittableCandidate,
};

use super::plan::{BootstrapCoin, LadderDeficit, PlannerLadderRow};

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

/// Ladder deficits and mixed-split output amounts from shape context.
#[must_use]
pub(crate) fn collect_bootstrap_ladder_deficits(
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
        deficits.push(LadderDeficit::new(size, required, current));
        output_amounts.extend(std::iter::repeat_n(
            size,
            usize::try_from(deficit).expect("deficit is positive"),
        ));
    }
    (deficits, output_amounts)
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
        .filter(|coin| coin.is_spendable())
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
