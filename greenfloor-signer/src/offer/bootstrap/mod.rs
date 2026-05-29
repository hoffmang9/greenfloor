//! Bootstrap mixed-output planner and phase policy for offer denomination preflight.

mod phase;
mod planner;

pub use phase::{bootstrap_early_phase, bootstrap_executed_phase, BootstrapPhaseSnapshot};
pub use planner::{
    plan_bootstrap_mixed_outputs, BootstrapCoin, BootstrapPlan, BootstrapPlanOutcome,
    LadderDeficit, PlannerLadderRow,
};
