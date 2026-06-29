//! Bootstrap phase status/reason mapping after planner evaluation or post-split replan.

use super::plan::{BootstrapCoin, BootstrapPlanOutcome, PlannerLadderRow};
use super::shape_policy::{
    bootstrap_preflight_deferred_to_coin_ops, offer_bootstrap_primary_row_complete,
};

#[cfg(test)]
mod tests;

/// Typed bootstrap phase status for offer-creation gating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapPhaseStatus {
    Failed,
    Executed,
    Skipped,
}

impl BootstrapPhaseStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::Executed => "executed",
            Self::Skipped => "skipped",
        }
    }
}

/// Manager-visible bootstrap phase fields (status / reason / ready).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapPhaseSnapshot {
    pub status: BootstrapPhaseStatus,
    pub reason: String,
    pub ready: bool,
}

fn phase_snapshot(
    status: BootstrapPhaseStatus,
    reason: impl Into<String>,
    ready: bool,
) -> BootstrapPhaseSnapshot {
    BootstrapPhaseSnapshot {
        status,
        reason: reason.into(),
        ready,
    }
}

#[derive(Debug, Clone, Copy)]
enum OutcomeReasonContext {
    Executed,
    WaitEvent(BootstrapWaitStepKind),
}

fn outcome_reason(context: OutcomeReasonContext, outcome: &BootstrapPlanOutcome) -> (String, bool) {
    if matches!(
        (context, outcome),
        (
            OutcomeReasonContext::WaitEvent(BootstrapWaitStepKind::AfterCombine),
            BootstrapPlanOutcome::NeedsShape(_)
        )
    ) {
        return ("combine_step_complete".to_string(), false);
    }
    match outcome {
        BootstrapPlanOutcome::Ready => ("bootstrap_submitted".to_string(), true),
        BootstrapPlanOutcome::CannotFund {
            total_output_amount,
        } => (
            format!(
                "bootstrap_submitted:still_underfunded:total_output_amount={total_output_amount}"
            ),
            false,
        ),
        BootstrapPlanOutcome::NeedsShape(_) => (
            if outcome.combine_first_pending() {
                "bootstrap_submitted:still_needs_combine".to_string()
            } else {
                "bootstrap_submitted:still_needs_split".to_string()
            },
            false,
        ),
        BootstrapPlanOutcome::InvalidLadder => (
            "bootstrap_submitted:still_invalid_ladder".to_string(),
            false,
        ),
        BootstrapPlanOutcome::InvalidCoins => {
            ("bootstrap_submitted:still_invalid_coins".to_string(), false)
        }
    }
}

fn after_combine_wait_complete_outcome(
    ctx: BootstrapWaitContext<'_>,
    outcome: &BootstrapPlanOutcome,
) -> Option<BootstrapPlanOutcome> {
    if outcome.combine_first_pending() {
        return None;
    }
    if offer_bootstrap_primary_row_complete(
        ctx.combine_target_amount,
        outcome,
        ctx.ladder_entries,
        ctx.spendable_coins,
    ) {
        return Some(BootstrapPlanOutcome::Ready);
    }
    match outcome {
        BootstrapPlanOutcome::Ready | BootstrapPlanOutcome::NeedsShape(_) => Some(outcome.clone()),
        _ => None,
    }
}

fn after_split_wait_complete_outcome(
    outcome: &BootstrapPlanOutcome,
    observed_on_chain_update: bool,
) -> Option<BootstrapPlanOutcome> {
    if matches!(outcome, BootstrapPlanOutcome::Ready)
        || (observed_on_chain_update && !outcome.combine_first_pending())
    {
        Some(outcome.clone())
    } else {
        None
    }
}

fn skipped_already_ready_snapshot() -> BootstrapPhaseSnapshot {
    phase_snapshot(BootstrapPhaseStatus::Skipped, "already_ready", false)
}

/// Map a planner outcome to an early bootstrap phase snapshot, if mixed-split should not run.
#[must_use]
pub fn bootstrap_early_phase(
    outcome: &BootstrapPlanOutcome,
    ladder_entries: &[PlannerLadderRow],
    spendable_coins: &[BootstrapCoin],
) -> Option<BootstrapPhaseSnapshot> {
    if bootstrap_preflight_deferred_to_coin_ops(outcome, ladder_entries, spendable_coins)
        || matches!(outcome, BootstrapPlanOutcome::Ready)
    {
        return Some(skipped_already_ready_snapshot());
    }
    match outcome {
        BootstrapPlanOutcome::NeedsShape(_) => None,
        BootstrapPlanOutcome::CannotFund {
            total_output_amount,
        } => Some(phase_snapshot(
            BootstrapPhaseStatus::Skipped,
            format!("bootstrap_underfunded:total_output_amount={total_output_amount}"),
            false,
        )),
        BootstrapPlanOutcome::InvalidLadder => Some(phase_snapshot(
            BootstrapPhaseStatus::Failed,
            "bootstrap_invalid_ladder",
            false,
        )),
        BootstrapPlanOutcome::InvalidCoins => Some(phase_snapshot(
            BootstrapPhaseStatus::Failed,
            "bootstrap_invalid_coins",
            false,
        )),
        BootstrapPlanOutcome::Ready => unreachable!(),
    }
}

/// Map a post-split replan outcome to executed-phase status/reason/ready.
#[must_use]
pub fn bootstrap_executed_phase(remaining: &BootstrapPlanOutcome) -> BootstrapPhaseSnapshot {
    let (reason, ready) = outcome_reason(OutcomeReasonContext::Executed, remaining);
    phase_snapshot(BootstrapPhaseStatus::Executed, reason, ready)
}

/// Which bootstrap shape step a confirmation wait is polling for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BootstrapWaitStepKind {
    AfterCombine,
    AfterSplit,
}

/// Result of evaluating one bootstrap shape wait poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BootstrapWaitResolution {
    Continue,
    Complete(BootstrapPlanOutcome),
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

#[must_use]
pub(crate) fn resolve_bootstrap_wait_poll(
    poll: BootstrapWaitPoll<'_>,
    outcome: &BootstrapPlanOutcome,
    observed_on_chain_update: bool,
) -> BootstrapWaitResolution {
    let completed = match poll {
        BootstrapWaitPoll::AfterCombine(ctx) => after_combine_wait_complete_outcome(ctx, outcome),
        BootstrapWaitPoll::AfterSplit => {
            after_split_wait_complete_outcome(outcome, observed_on_chain_update)
        }
    };
    match completed {
        Some(outcome) => BootstrapWaitResolution::Complete(outcome),
        None => BootstrapWaitResolution::Continue,
    }
}

/// Manager/operator metadata for a completed bootstrap shape wait event.
#[must_use]
pub(crate) fn bootstrap_wait_event_metadata(
    step: BootstrapWaitStepKind,
    outcome: &BootstrapPlanOutcome,
) -> (bool, String) {
    let (reason, ready) = outcome_reason(OutcomeReasonContext::WaitEvent(step), outcome);
    (ready, reason)
}
