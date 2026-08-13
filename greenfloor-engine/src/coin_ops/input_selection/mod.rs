//! Auto split/combine input selection for CLI and daemon coin ops.

mod auto_split;
mod combine_inputs;
#[cfg(test)]
mod combine_prereq_plan;
mod types;

#[cfg(test)]
mod tests;

pub use auto_split::{
    plan_cli_auto_split_selection, plan_daemon_auto_split_selection,
    plan_daemon_low_watermark_split,
};
pub use combine_inputs::plan_exact_amount_combine_inputs;
#[cfg(test)]
use combine_prereq_plan::build_combine_prereq_plan;
pub use types::{
    CliSplitSelection, DaemonAutoSplitParams, SplitAutoSelectPlan, SplitCoinPlan, SplitSkipReason,
    SubCatChangeSkipData,
};
