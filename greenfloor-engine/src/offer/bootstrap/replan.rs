//! Post-combine replan policy for bootstrap shape execution.

use super::plan::{BootstrapCoin, BootstrapPlan, BootstrapPlanOutcome, PlannerLadderRow};
use super::shape_policy::offer_bootstrap_primary_row_complete;

/// Next step after replanning inventory post-combine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BootstrapReplanAfterCombine {
    ContinueSplit(BootstrapPlan),
    Complete(BootstrapPlanOutcome),
}

/// Decide whether bootstrap should split, stop with a pending executed outcome, or finish.
#[must_use]
pub(crate) fn bootstrap_replan_after_combine(
    combine_target_amount: i64,
    replanned: BootstrapPlanOutcome,
    ladder_entries: &[PlannerLadderRow],
    spendable_coins: &[BootstrapCoin],
) -> BootstrapReplanAfterCombine {
    if offer_bootstrap_primary_row_complete(
        combine_target_amount,
        &replanned,
        ladder_entries,
        spendable_coins,
    ) {
        return BootstrapReplanAfterCombine::Complete(BootstrapPlanOutcome::Ready);
    }
    match replanned {
        BootstrapPlanOutcome::Ready
        | BootstrapPlanOutcome::CannotFund { .. }
        | BootstrapPlanOutcome::InvalidLadder
        | BootstrapPlanOutcome::InvalidCoins => BootstrapReplanAfterCombine::Complete(replanned),
        BootstrapPlanOutcome::NeedsShape(plan) if plan.requires_combine_first() => {
            // Still need another combine-first step for the primary row (for example 100@2).
            BootstrapReplanAfterCombine::Complete(BootstrapPlanOutcome::NeedsShape(plan))
        }
        BootstrapPlanOutcome::NeedsShape(plan) => BootstrapReplanAfterCombine::ContinueSplit(plan),
    }
}

#[cfg(test)]
mod tests {
    use super::{bootstrap_replan_after_combine, BootstrapReplanAfterCombine};
    use crate::offer::bootstrap::test_fixtures::{bootstrap_coin as coin, ladder_row as row};
    use crate::offer::bootstrap::{
        plan_bootstrap_mixed_outputs, BaseUnits, BootstrapCombineContext, BootstrapPlanOutcome,
    };

    #[test]
    fn replan_continues_split_for_non_ladder_combine_product() {
        let ladder = vec![row(10, 3, 0)];
        let spendable = vec![coin("combined", 30)];
        let replanned = plan_bootstrap_mixed_outputs(
            &ladder,
            &spendable,
            5,
            &BootstrapCombineContext::for_tests(),
        );
        assert!(matches!(
            bootstrap_replan_after_combine(30, replanned, &ladder, &spendable),
            BootstrapReplanAfterCombine::ContinueSplit(_)
        ));
    }

    #[test]
    fn replan_reports_underfunded_for_second_primary_row_with_existing_hundred() {
        let ladder = vec![row(100, 2, 0)];
        let spendable = vec![
            coin("first", 100),
            coin("fifty", 50),
            coin("forty", 40),
            coin("thirty_a", 30),
            coin("thirty_b", 30),
        ];
        let replanned = plan_bootstrap_mixed_outputs(
            &ladder,
            &spendable,
            5,
            &BootstrapCombineContext::for_tests(),
        );
        assert!(matches!(
            bootstrap_replan_after_combine(100, replanned, &ladder, &spendable),
            BootstrapReplanAfterCombine::Complete(BootstrapPlanOutcome::CannotFund {
                total_output_amount: 100
            })
        ));
    }

    #[test]
    fn replan_still_needs_combine_when_replan_is_combine_first() {
        use crate::offer::bootstrap::{
            BootstrapCombineInputs, BootstrapFundingSource, BootstrapPlan,
        };

        let ladder = vec![row(100, 2, 0)];
        let spendable = vec![coin("first", 100)];
        let replanned = BootstrapPlanOutcome::NeedsShape(BootstrapPlan {
            funding: BootstrapFundingSource::CombineFirst(BootstrapCombineInputs {
                input_coin_ids: vec!["a".repeat(64), "b".repeat(64)],
                selected_total: BaseUnits::new(105),
                target_amount: BaseUnits::new(100),
                exact_match: false,
                cap_applied: true,
            }),
            output_amounts_base_units: vec![100],
            total_output_amount: 100,
            change_amount: 5,
            deficits: vec![crate::offer::bootstrap::LadderDeficit {
                size_base_units: 100,
                required_count: 2,
                current_count: 1,
                deficit_count: 1,
            }],
        });
        assert!(matches!(
            bootstrap_replan_after_combine(100, replanned, &ladder, &spendable),
            BootstrapReplanAfterCombine::Complete(BootstrapPlanOutcome::NeedsShape(_))
        ));
    }
}
