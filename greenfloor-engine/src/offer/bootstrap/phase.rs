//! Bootstrap phase status/reason mapping after planner evaluation or post-split replan.

use super::planner::BootstrapPlanOutcome;

/// Manager-visible bootstrap phase fields (status / reason / ready).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapPhaseSnapshot {
    pub status: &'static str,
    pub reason: String,
    pub ready: bool,
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
        BootstrapPlanOutcome::NeedsSplit(_) => None,
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
        BootstrapPlanOutcome::NeedsSplit(_) => BootstrapPhaseSnapshot {
            status: "executed",
            reason: "bootstrap_submitted:still_needs_split".to_string(),
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
    use super::{bootstrap_early_phase, bootstrap_executed_phase};
    use crate::offer::bootstrap::{
        plan_bootstrap_mixed_outputs, BootstrapCoin, BootstrapPlanOutcome, PlannerLadderRow,
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
            amount,
        }
    }

    #[test]
    fn early_phase_skips_when_needs_split() {
        let ladder = vec![row(10, 2, 0)];
        let spendable = vec![coin("coin-big", 100)];
        let outcome = plan_bootstrap_mixed_outputs(&ladder, &spendable);
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
}
