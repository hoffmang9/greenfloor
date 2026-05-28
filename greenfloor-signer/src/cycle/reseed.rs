use super::strategy::{evaluate_market, MarketState, PlannedAction, StrategyConfig};

/// Why reseed gap injection did not produce actions (Python logging labels).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReseedSkipReason {
    StrategyActionsPresent,
    ActiveOfferTargetsSatisfied,
    NoSeedCandidates,
    MissingSizesNoSeedTemplate,
    ReseedZeroRepeatFiltered,
}

impl ReseedSkipReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::StrategyActionsPresent => "strategy_actions_present",
            Self::ActiveOfferTargetsSatisfied => "active_offer_targets_satisfied",
            Self::NoSeedCandidates => "no_seed_candidates",
            Self::MissingSizesNoSeedTemplate => "missing_sizes_no_seed_template",
            Self::ReseedZeroRepeatFiltered => "reseed_zero_repeat_filtered",
        }
    }
}

/// Stable label list for Python `ReseedSkipReason` parity tests and PyO3 FFI.
pub fn reseed_skip_reason_labels() -> Vec<&'static str> {
    vec![
        ReseedSkipReason::StrategyActionsPresent.label(),
        ReseedSkipReason::ActiveOfferTargetsSatisfied.label(),
        ReseedSkipReason::NoSeedCandidates.label(),
        ReseedSkipReason::MissingSizesNoSeedTemplate.label(),
        ReseedSkipReason::ReseedZeroRepeatFiltered.label(),
    ]
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReseedGapPlan {
    pub actions: Vec<PlannedAction>,
    pub skip_reason: Option<ReseedSkipReason>,
    pub missing_by_size: std::collections::BTreeMap<i64, i64>,
}

fn missing_counts_by_size(
    active_counts_by_size: &std::collections::BTreeMap<i64, i64>,
    target_counts_by_size: &std::collections::BTreeMap<i64, i64>,
) -> std::collections::BTreeMap<i64, i64> {
    target_counts_by_size
        .iter()
        .map(|(size, target)| {
            let active = active_counts_by_size.get(size).copied().unwrap_or(0);
            (*size, (*target - active).max(0))
        })
        .collect()
}

fn empty_market_state(xch_price_usd: Option<f64>) -> MarketState {
    MarketState {
        ones: 0,
        tens: 0,
        hundreds: 0,
        xch_price_usd,
        bucket_counts_by_size: None,
    }
}

/// Plan offer-size-gap reseed actions when the ordinary planner returned nothing.
///
/// Callers supply active/target counts from SQLite. Seed templates are derived
/// internally via [`evaluate_market`] on an empty bucket state.
pub fn plan_reseed_actions_from_gap(
    strategy_actions: &[PlannedAction],
    active_counts_by_size: &std::collections::BTreeMap<i64, i64>,
    target_counts_by_size: &std::collections::BTreeMap<i64, i64>,
    config: &StrategyConfig,
    xch_price_usd: Option<f64>,
) -> ReseedGapPlan {
    let missing_by_size = missing_counts_by_size(active_counts_by_size, target_counts_by_size);

    if !strategy_actions.is_empty() {
        return ReseedGapPlan {
            actions: strategy_actions.to_vec(),
            skip_reason: Some(ReseedSkipReason::StrategyActionsPresent),
            missing_by_size,
        };
    }

    if missing_by_size.values().copied().sum::<i64>() <= 0 {
        return ReseedGapPlan {
            actions: Vec::new(),
            skip_reason: Some(ReseedSkipReason::ActiveOfferTargetsSatisfied),
            missing_by_size,
        };
    }

    let seed_candidates = evaluate_market(&empty_market_state(xch_price_usd), config);
    if seed_candidates.is_empty() {
        return ReseedGapPlan {
            actions: Vec::new(),
            skip_reason: Some(ReseedSkipReason::NoSeedCandidates),
            missing_by_size,
        };
    }

    let mut one_per_size: std::collections::BTreeMap<i64, &PlannedAction> =
        std::collections::BTreeMap::new();
    for candidate in &seed_candidates {
        one_per_size.entry(candidate.size).or_insert(candidate);
    }

    let mut reseed_actions = Vec::new();
    for size in one_per_size.keys() {
        let missing = missing_by_size.get(size).copied().unwrap_or(0);
        if missing <= 0 {
            continue;
        }
        let template = one_per_size[size];
        reseed_actions.push(PlannedAction {
            size: template.size,
            repeat: missing,
            pair: template.pair.clone(),
            expiry_unit: template.expiry_unit.clone(),
            expiry_value: template.expiry_value,
            cancel_after_create: template.cancel_after_create,
            reason: "offer_size_gap_reseed".to_string(),
            target_spread_bps: template.target_spread_bps,
            side: template.side.clone(),
        });
    }

    if reseed_actions.is_empty() {
        return ReseedGapPlan {
            actions: Vec::new(),
            skip_reason: Some(ReseedSkipReason::MissingSizesNoSeedTemplate),
            missing_by_size,
        };
    }

    reseed_actions.retain(|action| action.repeat > 0);
    if reseed_actions.is_empty() {
        return ReseedGapPlan {
            actions: Vec::new(),
            skip_reason: Some(ReseedSkipReason::ReseedZeroRepeatFiltered),
            missing_by_size,
        };
    }

    ReseedGapPlan {
        actions: reseed_actions,
        skip_reason: None,
        missing_by_size,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn sample_config() -> StrategyConfig {
        StrategyConfig {
            pair: "xch".to_string(),
            ones_target: 5,
            tens_target: 2,
            hundreds_target: 1,
            target_spread_bps: None,
            min_xch_price_usd: None,
            max_xch_price_usd: None,
            offer_expiry_minutes: None,
            target_counts_by_size: None,
        }
    }

    #[test]
    fn reseed_skip_reason_labels_are_unique_and_complete() {
        let labels = reseed_skip_reason_labels();
        assert_eq!(labels.len(), 5);
        let unique: std::collections::BTreeSet<_> = labels.iter().copied().collect();
        assert_eq!(unique.len(), labels.len());
    }

    #[test]
    fn plan_reseed_skips_when_strategy_actions_present() {
        let config = sample_config();
        let existing = vec![PlannedAction {
            size: 1,
            repeat: 1,
            pair: "xch".to_string(),
            expiry_unit: "minutes".to_string(),
            expiry_value: 10,
            cancel_after_create: true,
            reason: "below_target".to_string(),
            target_spread_bps: None,
            side: "sell".to_string(),
        }];
        let plan = plan_reseed_actions_from_gap(
            &existing,
            &BTreeMap::new(),
            &BTreeMap::from([(1, 5)]),
            &config,
            Some(30.0),
        );
        assert_eq!(plan.actions, existing);
        assert_eq!(
            plan.skip_reason,
            Some(ReseedSkipReason::StrategyActionsPresent)
        );
        assert_eq!(plan.missing_by_size.get(&1), Some(&5));
    }

    #[test]
    fn plan_reseed_injects_gap_actions_for_empty_planner() {
        let config = sample_config();
        let targets = BTreeMap::from([(1, 5), (10, 2), (100, 1)]);
        let plan = plan_reseed_actions_from_gap(&[], &BTreeMap::new(), &targets, &config, Some(30.0));
        assert!(plan.skip_reason.is_none());
        assert_eq!(plan.actions.len(), 3);
        assert!(plan
            .actions
            .iter()
            .all(|action| action.reason == "offer_size_gap_reseed"));
        assert_eq!(
            plan.actions
                .iter()
                .map(|action| (action.size, action.repeat))
                .collect::<Vec<_>>(),
            vec![(1, 5), (10, 2), (100, 1)]
        );
        assert_eq!(plan.missing_by_size, targets);
    }

    #[test]
    fn plan_reseed_skips_when_targets_satisfied() {
        let config = sample_config();
        let active = BTreeMap::from([(1, 5), (10, 2), (100, 1)]);
        let targets = BTreeMap::from([(1, 5), (10, 2), (100, 1)]);
        let plan = plan_reseed_actions_from_gap(&[], &active, &targets, &config, Some(30.0));
        assert!(plan.actions.is_empty());
        assert_eq!(
            plan.skip_reason,
            Some(ReseedSkipReason::ActiveOfferTargetsSatisfied)
        );
        assert!(plan.missing_by_size.values().all(|missing| *missing == 0));
    }

    #[test]
    fn plan_reseed_partial_gap_refills_missing_sizes_only() {
        let config = sample_config();
        let active = BTreeMap::from([(1, 2)]);
        let targets = BTreeMap::from([(1, 5), (10, 2), (100, 1)]);
        let plan = plan_reseed_actions_from_gap(&[], &active, &targets, &config, Some(30.0));
        assert!(plan.skip_reason.is_none());
        assert_eq!(
            plan.actions
                .iter()
                .map(|action| (action.size, action.repeat))
                .collect::<Vec<_>>(),
            vec![(1, 3), (10, 2), (100, 1)]
        );
    }
}
