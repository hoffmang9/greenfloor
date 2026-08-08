//! Bootstrap mixed-output planner and phase policy for offer denomination preflight.

mod amounts;
mod combine_plan;
mod gate;
mod phase;
mod plan;
mod planner;
mod replan;
mod shape_policy;

#[cfg(test)]
mod test_fixtures;

pub(crate) use amounts::bootstrap_combine_vault_outputs_as_mojos;
#[cfg(test)]
pub use amounts::{
    bootstrap_combine_vault_outputs, bootstrap_mixed_split_output_mojos,
    bootstrap_overshoot_change_mojos, plan_amount_to_mojos,
};
pub use amounts::{vault_output_mojos_from_plan_amounts, PlanAmount};
pub use combine_plan::BootstrapCombineContext;
pub(crate) use gate::bootstrap_offer_gate_for_status;
pub use gate::bootstrap_phase_snapshot_block_error;
pub use phase::{
    bootstrap_early_phase, bootstrap_executed_phase, BootstrapPhaseSnapshot, BootstrapPhaseStatus,
};
pub(crate) use phase::{
    bootstrap_wait_event_metadata, resolve_bootstrap_wait_poll, BootstrapWaitContext,
    BootstrapWaitPoll, BootstrapWaitResolution, BootstrapWaitStepKind,
};
pub use plan::{BootstrapCoin, BootstrapPlan, BootstrapPlanOutcome, PlannerLadderRow};
pub use planner::plan_bootstrap_mixed_outputs;
pub(crate) use replan::{bootstrap_replan_after_combine, BootstrapReplanAfterCombine};

#[cfg(test)]
pub(crate) use shape_policy::{
    bootstrap_preflight_deferred_to_coin_ops, offer_bootstrap_primary_row_complete,
};
