use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::dispatch::{expand_inputs_by_repeat, PlannedActionInput};
use super::dispatch::{reservation_request_for_managed_offer, SpendableAssetProfile};
use super::managed::{prepare_parallel_managed_submission_decision, ParallelSubmissionDecision};
use super::strategy::PlannedAction;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParallelActionReservationInput {
    pub submit_index: usize,
    pub side: String,
    pub size_base_units: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParallelReservationContext {
    pub base_asset_id: String,
    pub quote_asset_id: String,
    pub fee_asset_id: String,
    pub fee_amount_mojos: i64,
    pub base_unit_mojo_multiplier: i64,
    pub quote_unit_mojo_multiplier: i64,
    pub quote_price: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ParallelReservationEntry {
    submit_index: usize,
    requested_amounts: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ParallelReservationPrep {
    entries: Vec<ParallelReservationEntry>,
    asset_ids: Vec<String>,
}

fn build_parallel_reservation_prep(
    actions: &[ParallelActionReservationInput],
    ctx: &ParallelReservationContext,
) -> ParallelReservationPrep {
    let mut entries = Vec::with_capacity(actions.len());
    let mut asset_ids = BTreeSet::new();
    for action in actions {
        let requested_amounts = reservation_request_for_managed_offer(
            &action.side,
            action.size_base_units,
            &ctx.base_asset_id,
            &ctx.quote_asset_id,
            ctx.base_unit_mojo_multiplier,
            ctx.quote_unit_mojo_multiplier,
            ctx.quote_price,
            &ctx.fee_asset_id,
            ctx.fee_amount_mojos,
        );
        for asset_id in requested_amounts.keys() {
            asset_ids.insert(asset_id.clone());
        }
        entries.push(ParallelReservationEntry {
            submit_index: action.submit_index,
            requested_amounts,
        });
    }
    ParallelReservationPrep {
        entries,
        asset_ids: asset_ids.into_iter().collect(),
    }
}

pub fn plan_parallel_managed_dispatch(
    actions: &[PlannedAction],
    ctx: &ParallelReservationContext,
    spendable_profiles: &BTreeMap<String, SpendableAssetProfile>,
) -> ParallelBatchPlan {
    let reservation_inputs: Vec<ParallelActionReservationInput> = actions
        .iter()
        .enumerate()
        .map(|(submit_index, action)| ParallelActionReservationInput {
            submit_index,
            side: action.side.clone(),
            size_base_units: action.size,
        })
        .collect();
    let prep = build_parallel_reservation_prep(&reservation_inputs, ctx);
    let mut plan = ParallelBatchPlan::default();
    for entry in &prep.entries {
        let decision = prepare_parallel_managed_submission_decision(
            &entry.requested_amounts,
            spendable_profiles,
        );
        match decision {
            ParallelSubmissionDecision::Skip { reason } => {
                plan.skip_items.push(ParallelSkipItem {
                    submit_index: entry.submit_index,
                    reason,
                });
            }
            ParallelSubmissionDecision::Proceed { available_amounts } => {
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

pub fn filter_planned_actions_with_positive_repeat(
    actions: &[PlannedAction],
) -> Vec<PlannedAction> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn sample_reservation_context() -> ParallelReservationContext {
        ParallelReservationContext {
            base_asset_id: "base_asset".to_string(),
            quote_asset_id: "quote_asset".to_string(),
            fee_asset_id: "xch_asset".to_string(),
            fee_amount_mojos: 0,
            base_unit_mojo_multiplier: 1000,
            quote_unit_mojo_multiplier: 1000,
            quote_price: 1.5,
        }
    }

    #[test]
    fn plan_parallel_managed_dispatch_splits_skip_and_queue() {
        let ctx = sample_reservation_context();
        let actions = vec![
            PlannedAction {
                size: 0,
                repeat: 1,
                pair: String::new(),
                expiry_unit: String::new(),
                expiry_value: 0,
                cancel_after_create: false,
                reason: String::new(),
                target_spread_bps: None,
                side: "sell".to_string(),
            },
            PlannedAction {
                size: 10,
                repeat: 1,
                pair: String::new(),
                expiry_unit: String::new(),
                expiry_value: 0,
                cancel_after_create: false,
                reason: String::new(),
                target_spread_bps: None,
                side: "sell".to_string(),
            },
        ];
        let spendable_profiles = BTreeMap::from([(
            "base_asset".to_string(),
            SpendableAssetProfile {
                total: 50000,
                max_single: 50000,
                max_single_known: true,
            },
        )]);
        let plan = plan_parallel_managed_dispatch(&actions, &ctx, &spendable_profiles);
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
