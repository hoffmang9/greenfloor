//! Bootstrap mixed-output planner and phase policy for offer denomination preflight.

mod amounts;
mod combine_inputs;
mod combine_plan;
mod combine_submit;
mod gate;
mod phase;
mod planner;

pub use amounts::{
    base_units_to_mojos, bootstrap_mixed_split_output_mojos, bootstrap_overshoot_change_mojos,
    BaseUnits, Mojos,
};
pub use combine_inputs::BootstrapCombineInputs;
pub use combine_plan::{build_bootstrap_combine_plan, BootstrapCombineContext};
pub(crate) use combine_submit::bootstrap_combine_vault_outputs;
pub(crate) use gate::{bootstrap_offer_gate_for_status, BootstrapPhaseStatus};
pub use phase::{bootstrap_early_phase, bootstrap_executed_phase, BootstrapPhaseSnapshot};
pub(crate) use phase::{
    bootstrap_wait_event_metadata, resolve_bootstrap_wait_poll, BootstrapWaitResolution,
    BootstrapWaitStepKind,
};
pub use planner::{
    plan_bootstrap_mixed_outputs, BootstrapCoin, BootstrapFundingSource, BootstrapPlan,
    BootstrapPlanOutcome, LadderDeficit, PlannerLadderRow,
};
