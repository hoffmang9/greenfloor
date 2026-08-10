//! Bootstrap adapter for remaining-shape ownership.
//!
//! Extracts path-local planner facts and asks [`crate::coin_ops::shape_ownership`] whether
//! bootstrap should yield. Offer-post bootstrap shapes the primary ladder row; sub-primary
//! buffer deficits are daemon coin-op scope.

use super::plan::{bootstrap_coin_amounts, BootstrapCoin, BootstrapPlanOutcome, PlannerLadderRow};
use super::planner::to_shape_rows;
use crate::coin_ops::shape::shape_context_for_rows;
use crate::coin_ops::shape_ownership::{bootstrap_handoff, BootstrapRemainingWork, ShapeHandoff};
use crate::coin_ops::shape_protection::{primary_row_satisfied, LadderShapeContext};

fn remaining_work_from_outcome(
    outcome: &BootstrapPlanOutcome,
    primary_size: i64,
) -> BootstrapRemainingWork {
    match outcome {
        BootstrapPlanOutcome::Ready => BootstrapRemainingWork::Ready,
        BootstrapPlanOutcome::CannotFund {
            total_output_amount,
        } => BootstrapRemainingWork::ShapeOrFund {
            remaining_total: *total_output_amount,
            primary_size,
            all_deficits_below_primary: true,
        },
        BootstrapPlanOutcome::NeedsShape(_) if outcome.combine_first_pending() => {
            BootstrapRemainingWork::CombineFirst
        }
        BootstrapPlanOutcome::NeedsShape(plan) => BootstrapRemainingWork::ShapeOrFund {
            remaining_total: plan.total_output_amount,
            primary_size,
            all_deficits_below_primary: plan
                .deficits
                .iter()
                .all(|deficit| deficit.size < primary_size),
        },
        _ => BootstrapRemainingWork::NonShape,
    }
}

fn shape_ctx_for(
    ladder_entries: &[PlannerLadderRow],
    spendable_coins: &[BootstrapCoin],
) -> LadderShapeContext {
    shape_context_for_rows(
        &to_shape_rows(ladder_entries),
        &bootstrap_coin_amounts(spendable_coins),
    )
}

/// Preflight handoff: yield means skip bootstrap shaping and let daemon coin ops backfill.
#[must_use]
pub(crate) fn bootstrap_preflight_handoff(
    outcome: &BootstrapPlanOutcome,
    ladder_entries: &[PlannerLadderRow],
    spendable_coins: &[BootstrapCoin],
) -> ShapeHandoff {
    let shape_ctx = shape_ctx_for(ladder_entries, spendable_coins);
    let Some(primary_size) = shape_ctx.primary_row_size() else {
        return if matches!(outcome, BootstrapPlanOutcome::Ready) {
            ShapeHandoff::Yield
        } else {
            ShapeHandoff::Keep
        };
    };
    bootstrap_handoff(
        shape_ctx.primary_row_satisfied(),
        remaining_work_from_outcome(outcome, primary_size),
    )
}

/// After-combine handoff: yield means the target primary row is complete for bootstrap.
#[must_use]
pub(crate) fn bootstrap_after_combine_handoff(
    combine_target_amount: i64,
    outcome: &BootstrapPlanOutcome,
    ladder_entries: &[PlannerLadderRow],
    spendable_coins: &[BootstrapCoin],
) -> ShapeHandoff {
    if !ladder_entries
        .iter()
        .any(|row| row.size == combine_target_amount)
    {
        return ShapeHandoff::Keep;
    }
    let shape_ctx = shape_ctx_for(ladder_entries, spendable_coins);
    bootstrap_handoff(
        primary_row_satisfied(
            combine_target_amount,
            &shape_ctx.protected_slots,
            &shape_ctx.exact_ladder_counts,
        ),
        remaining_work_from_outcome(outcome, combine_target_amount),
    )
}

#[cfg(test)]
mod tests {
    use super::{bootstrap_after_combine_handoff, bootstrap_preflight_handoff};
    use crate::coin_ops::shape::{CombineInputs, ShapeFunding};
    use crate::coin_ops::shape_ownership::{
        bootstrap_handoff, BootstrapRemainingWork, ShapeHandoff,
    };
    use crate::offer::bootstrap::test_fixtures::{
        bootstrap_coin as coin, ladder_deficit, ladder_row as row, plan_bootstrap,
    };
    use crate::offer::bootstrap::{
        bootstrap_replan_after_combine, BootstrapPlan, BootstrapPlanOutcome,
        BootstrapReplanAfterCombine,
    };

    #[test]
    fn after_combine_keeps_combine_first_for_second_primary_row() {
        let replanned = BootstrapPlanOutcome::NeedsShape(BootstrapPlan {
            funding: ShapeFunding::CombineFirst(CombineInputs {
                input_coin_ids: vec!["a".repeat(64), "b".repeat(64)],
                selected_total: 105,
                target_amount: 100,
                exact_match: false,
                cap_applied: true,
                selected_count_before_cap: 2,
                combine_input_cap: 5,
            }),
            output_amounts: vec![100],
            total_output_amount: 100,
            change_amount: 5,
            deficits: vec![ladder_deficit(100, 2, 1)],
        });
        let spendable = vec![coin("first", 100)];
        assert_eq!(
            bootstrap_after_combine_handoff(100, &replanned, &[row(100, 2, 0)], &spendable),
            ShapeHandoff::Keep
        );
        assert!(matches!(
            bootstrap_replan_after_combine(100, replanned, &[row(100, 2, 0)], &spendable),
            BootstrapReplanAfterCombine::Complete(BootstrapPlanOutcome::NeedsShape(_))
        ));
    }

    #[test]
    fn preflight_keeps_when_primary_row_still_underfunded() {
        let ladder = vec![row(100, 2, 0)];
        let after_one = vec![
            coin("first", 100),
            coin("fifty", 50),
            coin("forty", 40),
            coin("thirty_a", 30),
            coin("thirty_b", 30),
        ];
        let replanned = plan_bootstrap(&ladder, &after_one);
        assert_eq!(
            bootstrap_preflight_handoff(&replanned, &ladder, &after_one),
            ShapeHandoff::Keep
        );
    }

    #[test]
    fn after_combine_keeps_when_thirty_row_still_needs_split() {
        let ladder = vec![row(10, 3, 0)];
        let spendable = vec![coin("combined", 30)];
        let replanned = plan_bootstrap(&ladder, &spendable);
        assert_eq!(
            bootstrap_after_combine_handoff(30, &replanned, &ladder, &spendable),
            ShapeHandoff::Keep
        );
        assert!(matches!(replanned, BootstrapPlanOutcome::NeedsShape(_)));
    }

    #[test]
    fn preflight_yields_for_post_combine_split_plan() {
        use crate::test_support::eco181_bootstrap_inventory::{
            eco181_after_combine_coins, eco181_bootstrap_ladder,
        };

        let ladder = eco181_bootstrap_ladder();
        let coins = eco181_after_combine_coins();
        let outcome = plan_bootstrap(&ladder, &coins);
        assert_eq!(
            bootstrap_preflight_handoff(&outcome, &ladder, &coins),
            ShapeHandoff::Yield
        );
        assert_eq!(
            bootstrap_after_combine_handoff(100, &outcome, &ladder, &coins),
            ShapeHandoff::Yield
        );
    }

    #[test]
    fn invalid_ladder_work_keeps_at_ownership_seam() {
        assert_eq!(
            bootstrap_handoff(true, BootstrapRemainingWork::NonShape),
            ShapeHandoff::Keep
        );
    }

    #[test]
    fn preflight_ready_without_ladder_rows_yields() {
        assert_eq!(
            bootstrap_preflight_handoff(&BootstrapPlanOutcome::Ready, &[], &[]),
            ShapeHandoff::Yield
        );
    }
}
