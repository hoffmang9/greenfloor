//! Bootstrap phase status/reason mapping after planner evaluation or post-split replan.

use super::planner::BootstrapPlanOutcome;

/// Which bootstrap shape step a confirmation wait is polling for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BootstrapWaitStepKind {
    AfterCombine,
    AfterSplit,
}

/// Manager-visible bootstrap phase fields (status / reason / ready).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapPhaseSnapshot {
    pub status: &'static str,
    pub reason: String,
    pub ready: bool,
}

impl BootstrapPhaseSnapshot {
    /// Return manager bootstrap block reason text, or ``None`` when offer creation should continue.
    #[must_use]
    pub fn offer_creation_block_error(&self) -> Option<String> {
        super::gate::bootstrap_offer_gate_for_snapshot(self).block_error()
    }
}

/// Map a planner outcome to an early bootstrap phase snapshot, if mixed-split should not run.
#[must_use]
pub fn bootstrap_early_phase(outcome: &BootstrapPlanOutcome) -> Option<BootstrapPhaseSnapshot> {
    match outcome {
        BootstrapPlanOutcome::Ready => Some(BootstrapPhaseSnapshot {
            status: "skipped",
            reason: "already_ready".to_string(),
            ready: false,
        }),
        BootstrapPlanOutcome::CannotFund {
            total_output_amount,
        } => Some(BootstrapPhaseSnapshot {
            status: "skipped",
            reason: format!("bootstrap_underfunded:total_output_amount={total_output_amount}"),
            ready: false,
        }),
        BootstrapPlanOutcome::InvalidLadder => Some(BootstrapPhaseSnapshot {
            status: "failed",
            reason: "bootstrap_invalid_ladder".to_string(),
            ready: false,
        }),
        BootstrapPlanOutcome::InvalidCoins => Some(BootstrapPhaseSnapshot {
            status: "failed",
            reason: "bootstrap_invalid_coins".to_string(),
            ready: false,
        }),
        BootstrapPlanOutcome::NeedsShape(_) => None,
    }
}

/// Whether on-chain inventory satisfies the bootstrap wait step (planner re-evaluation).
#[must_use]
pub(crate) fn bootstrap_wait_step_satisfied(
    step: BootstrapWaitStepKind,
    outcome: &BootstrapPlanOutcome,
) -> bool {
    match step {
        BootstrapWaitStepKind::AfterCombine => match outcome {
            BootstrapPlanOutcome::Ready => true,
            BootstrapPlanOutcome::NeedsShape(plan) => !plan.requires_combine_first(),
            BootstrapPlanOutcome::CannotFund { .. }
            | BootstrapPlanOutcome::InvalidLadder
            | BootstrapPlanOutcome::InvalidCoins => false,
        },
        BootstrapWaitStepKind::AfterSplit => matches!(outcome, BootstrapPlanOutcome::Ready),
    }
}

/// Map a post-split replan outcome to executed-phase status/reason/ready.
#[must_use]
pub fn bootstrap_executed_phase(remaining: &BootstrapPlanOutcome) -> BootstrapPhaseSnapshot {
    match remaining {
        BootstrapPlanOutcome::Ready => BootstrapPhaseSnapshot {
            status: "executed",
            reason: "bootstrap_submitted".to_string(),
            ready: true,
        },
        BootstrapPlanOutcome::CannotFund {
            total_output_amount,
        } => BootstrapPhaseSnapshot {
            status: "executed",
            reason: format!(
                "bootstrap_submitted:still_underfunded:total_output_amount={total_output_amount}"
            ),
            ready: false,
        },
        BootstrapPlanOutcome::NeedsShape(plan) => BootstrapPhaseSnapshot {
            status: "executed",
            reason: if plan.requires_combine_first() {
                "bootstrap_submitted:still_needs_combine".to_string()
            } else {
                "bootstrap_submitted:still_needs_split".to_string()
            },
            ready: false,
        },
        BootstrapPlanOutcome::InvalidLadder => BootstrapPhaseSnapshot {
            status: "executed",
            reason: "bootstrap_submitted:still_invalid_ladder".to_string(),
            ready: false,
        },
        BootstrapPlanOutcome::InvalidCoins => BootstrapPhaseSnapshot {
            status: "executed",
            reason: "bootstrap_submitted:still_invalid_coins".to_string(),
            ready: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bootstrap_early_phase, bootstrap_executed_phase, bootstrap_wait_step_satisfied,
        BootstrapWaitStepKind,
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
    fn early_phase_skips_when_needs_split() {
        let ladder = vec![row(10, 2, 0)];
        let spendable = vec![coin("coin-big", 100)];
        let outcome = plan_bootstrap_mixed_outputs(
            &ladder,
            &spendable,
            5,
            &BootstrapCombineContext::for_tests(),
        );
        assert!(bootstrap_early_phase(&outcome).is_none());
    }

    #[test]
    fn executed_phase_reports_still_underfunded() {
        let remaining = BootstrapPlanOutcome::CannotFund {
            total_output_amount: 20,
        };
        let phase = bootstrap_executed_phase(&remaining);
        assert_eq!(phase.status, "executed");
        assert!(!phase.ready);
        assert!(phase
            .reason
            .contains("still_underfunded:total_output_amount=20"));
    }

    #[test]
    fn after_combine_wait_not_satisfied_on_cannot_fund_or_still_combine_first() {
        let ladder = vec![row(100, 1, 0)];
        let change_only = vec![coin("change", 5)];
        let outcome = plan_bootstrap_mixed_outputs(
            &ladder,
            &change_only,
            5,
            &BootstrapCombineContext::for_tests(),
        );
        assert!(!bootstrap_wait_step_satisfied(
            BootstrapWaitStepKind::AfterCombine,
            &outcome
        ));

        let cannot_fund = BootstrapPlanOutcome::CannotFund {
            total_output_amount: 100,
        };
        assert!(!bootstrap_wait_step_satisfied(
            BootstrapWaitStepKind::AfterCombine,
            &cannot_fund
        ));
    }

    #[test]
    fn after_combine_wait_satisfied_when_single_coin_split_plan_available() {
        let ladder = vec![row(100, 1, 0)];
        let spendable = vec![coin("combined", 100)];
        let outcome = plan_bootstrap_mixed_outputs(
            &ladder,
            &spendable,
            5,
            &BootstrapCombineContext::for_tests(),
        );
        assert!(bootstrap_wait_step_satisfied(
            BootstrapWaitStepKind::AfterCombine,
            &outcome
        ));
    }

    #[test]
    fn after_split_wait_satisfied_only_when_ready() {
        let ready = BootstrapPlanOutcome::Ready;
        assert!(bootstrap_wait_step_satisfied(
            BootstrapWaitStepKind::AfterSplit,
            &ready
        ));

        let ladder = vec![row(100, 2, 0)];
        let spendable = vec![coin("combined", 100)];
        let needs_split = plan_bootstrap_mixed_outputs(
            &ladder,
            &spendable,
            5,
            &BootstrapCombineContext::for_tests(),
        );
        assert!(!bootstrap_wait_step_satisfied(
            BootstrapWaitStepKind::AfterSplit,
            &needs_split
        ));
    }
}
