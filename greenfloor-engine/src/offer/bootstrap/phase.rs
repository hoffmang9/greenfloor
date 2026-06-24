//! Bootstrap phase status/reason mapping after planner evaluation or post-split replan.

use super::planner::{BootstrapCoin, BootstrapPlanOutcome, PlannerLadderRow};
use super::shape_policy::{
    bootstrap_preflight_deferred_to_coin_ops, offer_bootstrap_primary_row_complete,
};

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

fn bootstrap_already_ready_snapshot() -> BootstrapPhaseSnapshot {
    BootstrapPhaseSnapshot {
        status: "skipped",
        reason: "already_ready".to_string(),
        ready: false,
    }
}

/// Map a planner outcome to an early bootstrap phase snapshot, if mixed-split should not run.
#[must_use]
pub fn bootstrap_early_phase(
    outcome: &BootstrapPlanOutcome,
    ladder_entries: &[PlannerLadderRow],
    spendable_coins: &[BootstrapCoin],
) -> Option<BootstrapPhaseSnapshot> {
    if bootstrap_preflight_deferred_to_coin_ops(outcome, ladder_entries, spendable_coins) {
        return Some(bootstrap_already_ready_snapshot());
    }

    match outcome {
        BootstrapPlanOutcome::Ready => Some(bootstrap_already_ready_snapshot()),
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

/// Result of evaluating one bootstrap shape wait poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BootstrapWaitResolution {
    Continue,
    Complete(BootstrapPlanOutcome),
}

/// Whether a submitted split step has produced a settled post-split planner outcome.
#[must_use]
fn post_split_shape_step_settled(outcome: &BootstrapPlanOutcome) -> bool {
    match outcome {
        BootstrapPlanOutcome::Ready
        | BootstrapPlanOutcome::CannotFund { .. }
        | BootstrapPlanOutcome::InvalidLadder
        | BootstrapPlanOutcome::InvalidCoins => true,
        BootstrapPlanOutcome::NeedsShape(plan) => !plan.requires_combine_first(),
    }
}

/// Inventory and ladder context required for after-combine bootstrap wait polling.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BootstrapWaitContext<'a> {
    pub combine_target_amount: i64,
    pub ladder_entries: &'a [PlannerLadderRow],
    pub spendable_coins: &'a [BootstrapCoin],
}

/// Which bootstrap shape wait poll is being evaluated.
#[derive(Debug, Clone, Copy)]
pub(crate) enum BootstrapWaitPoll<'a> {
    AfterCombine(BootstrapWaitContext<'a>),
    AfterSplit,
}

/// Decide whether a bootstrap wait poll should continue or return its planner outcome.
///
/// After combine, completion follows planner semantics so transient `CannotFund` from
/// partial change coins does not exit early. Once the combine target row is on-chain,
/// [`offer_bootstrap_primary_row_complete`] ends the wait even when buffer deficits remain.
/// After split, completion requires an on-chain inventory update plus a settled post-split
/// planner outcome.
#[must_use]
pub(crate) fn resolve_bootstrap_wait_poll(
    poll: BootstrapWaitPoll<'_>,
    outcome: &BootstrapPlanOutcome,
    observed_on_chain_update: bool,
) -> BootstrapWaitResolution {
    match poll {
        BootstrapWaitPoll::AfterCombine(ctx) => {
            let still_needs_combine = matches!(
                outcome,
                BootstrapPlanOutcome::NeedsShape(plan) if plan.requires_combine_first()
            );
            if !still_needs_combine
                && offer_bootstrap_primary_row_complete(
                    ctx.combine_target_amount,
                    outcome,
                    ctx.ladder_entries,
                    ctx.spendable_coins,
                )
            {
                return BootstrapWaitResolution::Complete(BootstrapPlanOutcome::Ready);
            }
            match outcome {
                BootstrapPlanOutcome::Ready => BootstrapWaitResolution::Complete(outcome.clone()),
                BootstrapPlanOutcome::NeedsShape(plan) if !plan.requires_combine_first() => {
                    BootstrapWaitResolution::Complete(outcome.clone())
                }
                BootstrapPlanOutcome::NeedsShape(_)
                | BootstrapPlanOutcome::CannotFund { .. }
                | BootstrapPlanOutcome::InvalidLadder
                | BootstrapPlanOutcome::InvalidCoins => BootstrapWaitResolution::Continue,
            }
        }
        BootstrapWaitPoll::AfterSplit => {
            if matches!(outcome, BootstrapPlanOutcome::Ready) {
                return BootstrapWaitResolution::Complete(outcome.clone());
            }
            if observed_on_chain_update && post_split_shape_step_settled(outcome) {
                return BootstrapWaitResolution::Complete(outcome.clone());
            }
            BootstrapWaitResolution::Continue
        }
    }
}

/// Manager/operator metadata for a completed bootstrap shape wait event.
#[must_use]
pub(crate) fn bootstrap_wait_event_metadata(
    step: BootstrapWaitStepKind,
    outcome: &BootstrapPlanOutcome,
) -> (bool, String) {
    match (step, outcome) {
        (_, BootstrapPlanOutcome::Ready) => (true, "bootstrap_submitted".to_string()),
        (BootstrapWaitStepKind::AfterCombine, BootstrapPlanOutcome::NeedsShape(_)) => {
            (false, "combine_step_complete".to_string())
        }
        _ => {
            let phase = bootstrap_executed_phase(outcome);
            (phase.ready, phase.reason)
        }
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
        bootstrap_early_phase, bootstrap_executed_phase, resolve_bootstrap_wait_poll,
        BootstrapWaitContext, BootstrapWaitPoll, BootstrapWaitResolution,
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

    fn after_combine_poll<'a>(
        combine_target_amount: i64,
        ladder: &'a [PlannerLadderRow],
        spendable: &'a [BootstrapCoin],
    ) -> BootstrapWaitPoll<'a> {
        BootstrapWaitPoll::AfterCombine(BootstrapWaitContext {
            combine_target_amount,
            ladder_entries: ladder,
            spendable_coins: spendable,
        })
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
        assert!(bootstrap_early_phase(&outcome, &ladder, &spendable).is_none());
    }

    #[test]
    fn early_phase_reports_already_ready() {
        let ladder = vec![row(10, 2, 0)];
        let spendable = vec![coin("coin-a", 10), coin("coin-b", 10)];
        let phase = bootstrap_early_phase(&BootstrapPlanOutcome::Ready, &ladder, &spendable)
            .expect("ready snapshot");
        assert_eq!(phase.status, "skipped");
        assert_eq!(phase.reason, "already_ready");
        assert!(!phase.ready);
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
    fn after_combine_wait_completes_when_combine_fully_shapes_ladder() {
        let ladder = vec![row(100, 1, 0)];
        let spendable = vec![coin("combined", 100)];
        let ready = BootstrapPlanOutcome::Ready;
        assert_eq!(
            resolve_bootstrap_wait_poll(
                after_combine_poll(100, &ladder, &spendable),
                &ready,
                false,
            ),
            BootstrapWaitResolution::Complete(ready)
        );
    }

    #[test]
    fn after_combine_wait_not_complete_on_cannot_fund_even_when_inventory_changed() {
        let ladder = vec![row(100, 1, 0)];
        let change_only = vec![coin("change", 5)];
        let outcome = plan_bootstrap_mixed_outputs(
            &ladder,
            &change_only,
            5,
            &BootstrapCombineContext::for_tests(),
        );
        assert_eq!(
            resolve_bootstrap_wait_poll(
                after_combine_poll(100, &ladder, &change_only),
                &outcome,
                true,
            ),
            BootstrapWaitResolution::Continue
        );

        let cannot_fund = BootstrapPlanOutcome::CannotFund {
            total_output_amount: 100,
        };
        assert_eq!(
            resolve_bootstrap_wait_poll(
                after_combine_poll(100, &ladder, &change_only),
                &cannot_fund,
                true,
            ),
            BootstrapWaitResolution::Continue
        );
    }

    #[test]
    fn after_combine_wait_completes_when_single_coin_split_plan_available() {
        let ladder = vec![row(100, 1, 0)];
        let spendable = vec![coin("combined", 100)];
        let outcome = plan_bootstrap_mixed_outputs(
            &ladder,
            &spendable,
            5,
            &BootstrapCombineContext::for_tests(),
        );
        assert!(matches!(
            resolve_bootstrap_wait_poll(
                after_combine_poll(100, &ladder, &spendable),
                &outcome,
                false,
            ),
            BootstrapWaitResolution::Complete(_)
        ));
    }

    #[test]
    fn after_combine_wait_completes_on_ready_outcome() {
        let ladder = vec![row(100, 1, 0)];
        let spendable = vec![coin("combined", 100)];
        assert_eq!(
            resolve_bootstrap_wait_poll(
                after_combine_poll(100, &ladder, &spendable),
                &BootstrapPlanOutcome::Ready,
                false,
            ),
            BootstrapWaitResolution::Complete(BootstrapPlanOutcome::Ready),
        );
    }

    #[test]
    fn after_combine_wait_continues_while_combine_first_still_pending() {
        use crate::test_support::eco181_bootstrap_inventory::{
            eco181_bootstrap_coins, eco181_bootstrap_ladder,
        };

        let ladder = eco181_bootstrap_ladder();
        let coins = eco181_bootstrap_coins();
        let outcome =
            plan_bootstrap_mixed_outputs(&ladder, &coins, 5, &BootstrapCombineContext::for_tests());
        let BootstrapPlanOutcome::NeedsShape(plan) = &outcome else {
            panic!("expected combine-first plan");
        };
        assert!(plan.requires_combine_first());
        assert_eq!(
            resolve_bootstrap_wait_poll(after_combine_poll(100, &ladder, &coins), &outcome, false,),
            BootstrapWaitResolution::Continue,
        );
    }

    #[test]
    fn after_split_wait_completes_on_ready_or_settled_inventory_update() {
        let ready = BootstrapPlanOutcome::Ready;
        assert_eq!(
            resolve_bootstrap_wait_poll(BootstrapWaitPoll::AfterSplit, &ready, false),
            BootstrapWaitResolution::Complete(ready)
        );

        let ladder = vec![row(100, 2, 0)];
        let spendable = vec![coin("combined", 100)];
        let needs_split = plan_bootstrap_mixed_outputs(
            &ladder,
            &spendable,
            5,
            &BootstrapCombineContext::for_tests(),
        );
        assert_eq!(
            resolve_bootstrap_wait_poll(BootstrapWaitPoll::AfterSplit, &needs_split, false),
            BootstrapWaitResolution::Continue
        );
        assert!(matches!(
            resolve_bootstrap_wait_poll(BootstrapWaitPoll::AfterSplit, &needs_split, true),
            BootstrapWaitResolution::Complete(_)
        ));

        let cannot_fund = BootstrapPlanOutcome::CannotFund {
            total_output_amount: 200,
        };
        assert!(matches!(
            resolve_bootstrap_wait_poll(BootstrapWaitPoll::AfterSplit, &cannot_fund, true),
            BootstrapWaitResolution::Complete(_)
        ));
    }

    #[test]
    fn after_split_wait_ignores_combine_first_inventory_updates() {
        let ladder = vec![row(100, 1, 0)];
        let fragmented = vec![
            coin("sixty", 60),
            coin("ten-a", 10),
            coin("ten-b", 10),
            coin("ten-c", 10),
            coin("ten-d", 10),
        ];
        let combine_first = plan_bootstrap_mixed_outputs(
            &ladder,
            &fragmented,
            5,
            &BootstrapCombineContext::for_tests(),
        );
        let BootstrapPlanOutcome::NeedsShape(plan) = &combine_first else {
            panic!("expected combine-first plan, got {combine_first:?}");
        };
        assert!(plan.requires_combine_first());
        assert_eq!(
            resolve_bootstrap_wait_poll(BootstrapWaitPoll::AfterSplit, &combine_first, true),
            BootstrapWaitResolution::Continue
        );
    }
}
