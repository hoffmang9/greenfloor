use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::dispatch::{expand_strategy_actions, PlannedActionInput};
use super::managed::{
    can_parallelize_managed_offers, prepare_parallel_managed_submission_decision,
    ParallelSubmissionDecision,
};
use super::dispatch::SpendableAssetProfile;
use super::strategy::PlannedAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrategyExecutionDispatch {
    Parallel,
    Sequential,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SequentialActionRoute {
    DryRunPlanned,
    Managed,
    Local,
    SkipNoProgram,
    SkipNoManagedBackend,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParallelSubmissionEntry {
    pub submit_index: usize,
    pub size: i64,
    pub side: String,
    pub requested_amounts: BTreeMap<String, i64>,
    pub spendable_profiles: BTreeMap<String, SpendableAssetProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParallelSkipItem {
    pub submit_index: usize,
    pub size: i64,
    pub side: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParallelQueueItem {
    pub submit_index: usize,
    pub size: i64,
    pub side: String,
    pub requested_amounts: BTreeMap<String, i64>,
    pub available_amounts: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ParallelBatchPlan {
    pub skip_items: Vec<ParallelSkipItem>,
    pub queue: Vec<ParallelQueueItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StrategyActionResultCounts {
    pub planned_count: i64,
    pub executed_count: i64,
}

pub fn select_strategy_execution_dispatch(
    signer_path_configured: bool,
    parallelism_enabled: bool,
    runtime_dry_run: bool,
    has_coordinator: bool,
) -> StrategyExecutionDispatch {
    if can_parallelize_managed_offers(
        signer_path_configured,
        parallelism_enabled,
        runtime_dry_run,
        has_coordinator,
    ) {
        StrategyExecutionDispatch::Parallel
    } else {
        StrategyExecutionDispatch::Sequential
    }
}

pub fn sequential_action_route(
    runtime_dry_run: bool,
    program_present: bool,
    managed_backend_available: bool,
) -> SequentialActionRoute {
    if runtime_dry_run {
        return SequentialActionRoute::DryRunPlanned;
    }
    if !program_present {
        return SequentialActionRoute::SkipNoProgram;
    }
    if managed_backend_available {
        SequentialActionRoute::Managed
    } else {
        SequentialActionRoute::Local
    }
}

pub fn filter_planned_actions_with_positive_repeat(actions: &[PlannedAction]) -> Vec<PlannedAction> {
    actions
        .iter()
        .filter(|action| action.repeat > 0)
        .cloned()
        .collect()
}

pub fn expand_planned_actions(actions: &[PlannedAction]) -> Vec<PlannedAction> {
    let inputs: Vec<PlannedActionInput> = actions
        .iter()
        .map(|action| PlannedActionInput {
            size: action.size,
            repeat: action.repeat,
            side: Some(action.side.clone()),
            pair: Some(action.pair.clone()),
            expiry_unit: Some(action.expiry_unit.clone()),
            expiry_value: Some(action.expiry_value),
            cancel_after_create: Some(action.cancel_after_create),
            reason: Some(action.reason.clone()),
            target_spread_bps: action.target_spread_bps,
        })
        .collect();
    expand_strategy_actions(&inputs)
        .into_iter()
        .map(|input| PlannedAction {
            size: input.size,
            repeat: 1,
            side: input.side.unwrap_or_else(|| "sell".to_string()),
            pair: input.pair.unwrap_or_default(),
            expiry_unit: input.expiry_unit.unwrap_or_default(),
            expiry_value: input.expiry_value.unwrap_or(0),
            cancel_after_create: input.cancel_after_create.unwrap_or(false),
            reason: input.reason.unwrap_or_default(),
            target_spread_bps: input.target_spread_bps,
        })
        .collect()
}

pub fn plan_parallel_submission_batch(entries: &[ParallelSubmissionEntry]) -> ParallelBatchPlan {
    let mut plan = ParallelBatchPlan::default();
    for entry in entries {
        let decision = prepare_parallel_managed_submission_decision(
            &entry.requested_amounts,
            &entry.spendable_profiles,
        );
        match decision {
            ParallelSubmissionDecision::Skip { reason } => {
                plan.skip_items.push(ParallelSkipItem {
                    submit_index: entry.submit_index,
                    size: entry.size,
                    side: entry.side.clone(),
                    reason,
                });
            }
            ParallelSubmissionDecision::Proceed {
                available_amounts,
            } => {
                plan.queue.push(ParallelQueueItem {
                    submit_index: entry.submit_index,
                    size: entry.size,
                    side: entry.side.clone(),
                    requested_amounts: entry.requested_amounts.clone(),
                    available_amounts,
                });
            }
        }
    }
    plan
}

pub fn aggregate_strategy_action_result_counts(
    planned_count: i64,
    item_statuses: &[&str],
) -> StrategyActionResultCounts {
    let executed_count = item_statuses
        .iter()
        .filter(|status| **status == "executed")
        .count() as i64;
    StrategyActionResultCounts {
        planned_count,
        executed_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn select_dispatch_parallel_when_eligible() {
        assert_eq!(
            select_strategy_execution_dispatch(true, true, false, true),
            StrategyExecutionDispatch::Parallel
        );
    }

    #[test]
    fn select_dispatch_sequential_when_dry_run() {
        assert_eq!(
            select_strategy_execution_dispatch(true, true, true, true),
            StrategyExecutionDispatch::Sequential
        );
    }

    #[test]
    fn sequential_route_dry_run() {
        assert_eq!(
            sequential_action_route(true, true, true),
            SequentialActionRoute::DryRunPlanned
        );
    }

    #[test]
    fn sequential_route_managed_when_backend_available() {
        assert_eq!(
            sequential_action_route(false, true, true),
            SequentialActionRoute::Managed
        );
    }

    #[test]
    fn sequential_route_local_without_managed_backend() {
        assert_eq!(
            sequential_action_route(false, true, false),
            SequentialActionRoute::Local
        );
    }

    #[test]
    fn plan_parallel_batch_splits_skip_and_queue() {
        let entries = vec![
            ParallelSubmissionEntry {
                submit_index: 0,
                size: 10,
                side: "sell".to_string(),
                requested_amounts: BTreeMap::new(),
                spendable_profiles: BTreeMap::new(),
            },
            ParallelSubmissionEntry {
                submit_index: 1,
                size: 1,
                side: "sell".to_string(),
                requested_amounts: BTreeMap::from([("asset_a".to_string(), 1000)]),
                spendable_profiles: BTreeMap::from([(
                    "asset_a".to_string(),
                    SpendableAssetProfile {
                        total: 5000,
                        max_single: 5000,
                        max_single_known: true,
                    },
                )]),
            },
        ];
        let plan = plan_parallel_submission_batch(&entries);
        assert_eq!(plan.skip_items.len(), 1);
        assert_eq!(plan.skip_items[0].submit_index, 0);
        assert_eq!(plan.queue.len(), 1);
        assert_eq!(plan.queue[0].submit_index, 1);
    }

    #[test]
    fn aggregate_counts_executed_items() {
        let counts = aggregate_strategy_action_result_counts(
            3,
            &["planned", "executed", "skipped"],
        );
        assert_eq!(counts.planned_count, 3);
        assert_eq!(counts.executed_count, 1);
    }
}
