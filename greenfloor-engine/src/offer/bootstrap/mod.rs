//! Bootstrap mixed-output planner and phase policy for offer denomination preflight.

mod gate;
mod phase;
mod planner;

pub(crate) use gate::{bootstrap_offer_gate_for_status, BootstrapPhaseStatus};
pub use phase::{bootstrap_early_phase, bootstrap_executed_phase, BootstrapPhaseSnapshot};
pub use planner::{
    plan_bootstrap_mixed_outputs, BootstrapCoin, BootstrapPlan, BootstrapPlanOutcome,
    LadderDeficit, PlannerLadderRow,
};
