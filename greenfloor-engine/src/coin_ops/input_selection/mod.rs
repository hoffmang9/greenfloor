//! Auto split/combine input selection for CLI and daemon coin ops.

mod auto_split;
mod combine_inputs;
mod combine_prereq_plan;
mod combine_selection;
mod types;

#[cfg(test)]
mod tests;

pub use auto_split::{
    plan_cli_auto_split_selection, plan_daemon_auto_split_selection,
    plan_daemon_low_watermark_split,
};
pub use combine_inputs::{plan_exact_amount_combine_inputs, plan_largest_combine_inputs};
pub use combine_prereq_plan::build_combine_prereq_plan;
pub(crate) use combine_selection::{select_combine_inputs_for_target_in, TargetAmountCoin};
pub use types::{
    CliSplitSelection, DaemonAutoSplitParams, SplitAutoSelectPlan, SplitCoinPlan,
    SplitCombinePrereqPlan, SplitSkipReason, SubCatChangeSkipData,
};
