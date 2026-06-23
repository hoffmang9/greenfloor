//! Bootstrap mixed-output planner and phase policy for offer denomination preflight.

mod combine_inputs;
mod gate;
mod phase;
mod planner;

pub use combine_inputs::BootstrapCombineInputs;
pub(crate) use gate::{bootstrap_offer_gate_for_status, BootstrapPhaseStatus};
pub use phase::{bootstrap_early_phase, bootstrap_executed_phase, BootstrapPhaseSnapshot};
pub use planner::{
    plan_bootstrap_mixed_outputs, BootstrapCoin, BootstrapFundingSource, BootstrapPlan,
    BootstrapPlanOutcome, LadderDeficit, PlannerLadderRow,
};
