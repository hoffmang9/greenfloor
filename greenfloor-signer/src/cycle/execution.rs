use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::dispatch::{expand_inputs_by_repeat, PlannedActionInput};
use super::dispatch::SpendableAssetProfile;
use super::managed::{prepare_parallel_managed_submission_decision, ParallelSubmissionDecision};
use super::strategy::PlannedAction;

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
    pub requested_amounts: BTreeMap<String, i64>,
    pub spendable_profiles: BTreeMap<String, SpendableAssetProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParallelSkipItem {
    pub submit_index: usize,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParallelQueueItem {
    pub submit_index: usize,
    pub requested_amounts: BTreeMap<String, i64>,
    pub available_amounts: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ParallelBatchPlan {
    pub skip_items: Vec<ParallelSkipItem>,
    pub queue: Vec<ParallelQueueItem>,
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
    expand_inputs_by_repeat(&inputs)
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
                    reason,
                });
            }
            ParallelSubmissionDecision::Proceed {
                available_amounts,
            } => {
                plan.queue.push(ParallelQueueItem {
                    submit_index: entry.submit_index,
                    requested_amounts: entry.requested_amounts.clone(),
                    available_amounts,
                });
            }
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

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
                requested_amounts: BTreeMap::new(),
                spendable_profiles: BTreeMap::new(),
            },
            ParallelSubmissionEntry {
                submit_index: 1,
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
    fn expand_planned_actions_sets_repeat_one_per_unit() {
        let actions = vec![PlannedAction {
            size: 10,
            repeat: 2,
            pair: "xch".to_string(),
            expiry_unit: "minutes".to_string(),
            expiry_value: 10,
            cancel_after_create: true,
            reason: "below_target".to_string(),
            target_spread_bps: None,
            side: "sell".to_string(),
        }];
        let expanded = expand_planned_actions(&actions);
        assert_eq!(expanded.len(), 2);
        assert!(expanded.iter().all(|action| action.repeat == 1));
        assert!(expanded.iter().all(|action| action.size == 10));
    }
}
