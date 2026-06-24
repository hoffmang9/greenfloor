//! Post-combine replan policy for bootstrap shape execution.

use super::planner::{BootstrapPlan, BootstrapPlanOutcome};

/// Next step after replanning inventory post-combine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BootstrapReplanAfterCombine {
    ContinueSplit(BootstrapPlan),
    Complete(BootstrapPlanOutcome),
}

/// True when a split plan would spend the just-completed combine product coin.
#[must_use]
pub(crate) fn shape_plan_cannibalizes_combine_target(
    combine_target_amount: i64,
    outcome: &BootstrapPlanOutcome,
) -> bool {
    let BootstrapPlanOutcome::NeedsShape(plan) = outcome else {
        return false;
    };
    if plan.requires_combine_first() {
        return false;
    }
    let source_size = plan.source_amount();
    if source_size != combine_target_amount {
        return false;
    }
    !plan
        .deficits
        .iter()
        .any(|deficit| deficit.size_base_units == source_size)
}

/// Decide whether bootstrap should split, stop with a pending executed outcome, or finish.
#[must_use]
pub(crate) fn bootstrap_replan_after_combine(
    combine_target_amount: i64,
    replanned: BootstrapPlanOutcome,
) -> BootstrapReplanAfterCombine {
    if shape_plan_cannibalizes_combine_target(combine_target_amount, &replanned) {
        return BootstrapReplanAfterCombine::Complete(replanned);
    }
    match replanned {
        BootstrapPlanOutcome::Ready
        | BootstrapPlanOutcome::CannotFund { .. }
        | BootstrapPlanOutcome::InvalidLadder
        | BootstrapPlanOutcome::InvalidCoins => BootstrapReplanAfterCombine::Complete(replanned),
        BootstrapPlanOutcome::NeedsShape(plan) if plan.requires_combine_first() => {
            BootstrapReplanAfterCombine::Complete(BootstrapPlanOutcome::NeedsShape(plan))
        }
        BootstrapPlanOutcome::NeedsShape(plan) => BootstrapReplanAfterCombine::ContinueSplit(plan),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bootstrap_replan_after_combine, shape_plan_cannibalizes_combine_target,
        BootstrapReplanAfterCombine,
    };
    use crate::offer::bootstrap::{
        plan_bootstrap_mixed_outputs, BaseUnits, BootstrapCoin, BootstrapCombineContext,
        BootstrapPlanOutcome, PlannerLadderRow,
    };

    fn row(size: i64, target: i64, buffer: i64) -> PlannerLadderRow {
        PlannerLadderRow {
            size_base_units: size,
            target_count: target,
            split_buffer_count: buffer,
        }
    }

    fn coin(id: &str, amount: i64) -> BootstrapCoin {
        BootstrapCoin {
            id: id.to_string(),
            amount: BaseUnits::new(amount),
        }
    }

    #[test]
    fn cannibalizing_split_is_detected_for_single_output_combine() {
        let ladder = vec![row(10, 2, 1), row(100, 1, 0)];
        let after_combine = vec![coin("combined", 100), coin("ten", 10)];
        let replanned = plan_bootstrap_mixed_outputs(
            &ladder,
            &after_combine,
            5,
            &BootstrapCombineContext::for_tests(),
        );
        assert!(shape_plan_cannibalizes_combine_target(100, &replanned));
    }

    #[test]
    fn cannibalizing_split_uses_combine_product_not_individual_outputs() {
        let ladder = vec![row(10, 3, 0)];
        let after_combine = vec![coin("combined", 30)];
        let replanned = plan_bootstrap_mixed_outputs(
            &ladder,
            &after_combine,
            5,
            &BootstrapCombineContext::for_tests(),
        );
        assert!(
            shape_plan_cannibalizes_combine_target(30, &replanned),
            "split from 30 BU combine product must defer: {replanned:?}"
        );
        assert!(
            !shape_plan_cannibalizes_combine_target(10, &replanned),
            "individual output sizes must not be used as combine fingerprint"
        );
    }

    #[test]
    fn replan_defers_cannibalizing_split() {
        let ladder = vec![row(10, 2, 1), row(100, 1, 0)];
        let after_combine = vec![coin("combined", 100), coin("ten", 10)];
        let replanned = plan_bootstrap_mixed_outputs(
            &ladder,
            &after_combine,
            5,
            &BootstrapCombineContext::for_tests(),
        );
        assert!(matches!(
            bootstrap_replan_after_combine(100, replanned),
            BootstrapReplanAfterCombine::Complete(BootstrapPlanOutcome::NeedsShape(_))
        ));
    }

    #[test]
    fn replan_continues_to_single_coin_split_when_inventory_has_combined_coin() {
        let ladder = vec![row(100, 1, 0)];
        let after_combine = vec![coin("combined", 100)];
        let replanned = plan_bootstrap_mixed_outputs(
            &ladder,
            &after_combine,
            5,
            &BootstrapCombineContext::for_tests(),
        );
        match bootstrap_replan_after_combine(100, replanned) {
            BootstrapReplanAfterCombine::ContinueSplit(plan) => {
                assert!(!plan.requires_combine_first());
                assert_eq!(plan.output_amounts_base_units, vec![100]);
            }
            BootstrapReplanAfterCombine::Complete(BootstrapPlanOutcome::Ready) => {}
            BootstrapReplanAfterCombine::Complete(other) => {
                panic!("unexpected replan outcome: {other:?}");
            }
        }
    }
}
