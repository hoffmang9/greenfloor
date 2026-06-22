//! Auto split/combine input selection for CLI and daemon coin ops.

mod auto_split;
mod combine_inputs;
mod combine_prereq_plan;
mod types;

#[cfg(test)]
mod tests;

pub use auto_split::{plan_cli_auto_split_selection, plan_daemon_auto_split_selection};
pub use combine_inputs::{plan_exact_amount_combine_inputs, plan_largest_combine_inputs};
pub use combine_prereq_plan::build_combine_prereq_plan;
pub use types::{
    CliSplitSelection, SplitAutoSelectPlan, SplitCoinPlan, SplitCombinePrereqPlan, SplitSkipReason,
    SubCatChangeSkipData,
};
