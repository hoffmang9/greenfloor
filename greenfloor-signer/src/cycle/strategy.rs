use serde::{Deserialize, Serialize};

const DEFAULT_OFFER_EXPIRY_MINUTES: i64 = 10;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketState {
    pub ones: i64,
    pub tens: i64,
    pub hundreds: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xch_price_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket_counts_by_size: Option<std::collections::BTreeMap<i64, i64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyConfig {
    pub pair: String,
    #[serde(default = "default_ones_target")]
    pub ones_target: i64,
    #[serde(default = "default_tens_target")]
    pub tens_target: i64,
    #[serde(default = "default_hundreds_target")]
    pub hundreds_target: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_spread_bps: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_xch_price_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_xch_price_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offer_expiry_minutes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_counts_by_size: Option<std::collections::BTreeMap<i64, i64>>,
}

fn default_ones_target() -> i64 {
    5
}

fn default_tens_target() -> i64 {
    2
}

fn default_hundreds_target() -> i64 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedAction {
    pub size: i64,
    pub repeat: i64,
    pub pair: String,
    pub expiry_unit: String,
    pub expiry_value: i64,
    pub cancel_after_create: bool,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_spread_bps: Option<i64>,
    #[serde(default = "default_side")]
    pub side: String,
}

fn default_side() -> String {
    "sell".to_string()
}

fn strategy_target_counts(config: &StrategyConfig) -> Vec<(i64, i64)> {
    if let Some(targets) = &config.target_counts_by_size {
        let mut entries: Vec<(i64, i64)> = targets
            .iter()
            .filter_map(|(size, target)| {
                let size = *size;
                let target = *target;
                if size > 0 && target >= 0 {
                    Some((size, target))
                } else {
                    None
                }
            })
            .collect();
        entries.sort_by_key(|entry| entry.0);
        return entries;
    }
    vec![
        (1, config.ones_target),
        (10, config.tens_target),
        (100, config.hundreds_target),
    ]
}

fn state_count_for_size(state: &MarketState, size: i64) -> i64 {
    if let Some(buckets) = &state.bucket_counts_by_size {
        return buckets.get(&size).copied().unwrap_or(0);
    }
    match size {
        1 => state.ones,
        10 => state.tens,
        100 => state.hundreds,
        _ => 0,
    }
}

pub fn evaluate_market(state: &MarketState, config: &StrategyConfig) -> Vec<PlannedAction> {
    let pair = config.pair.to_ascii_lowercase();
    if pair == "xch" {
        let Some(price) = state.xch_price_usd else {
            return Vec::new();
        };
        if price <= 0.0 {
            return Vec::new();
        }
        if let Some(min_price) = config.min_xch_price_usd {
            if price < min_price {
                return Vec::new();
            }
        }
        if let Some(max_price) = config.max_xch_price_usd {
            if price > max_price {
                return Vec::new();
            }
        }
    }

    let expiry_minutes = config
        .offer_expiry_minutes
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_OFFER_EXPIRY_MINUTES);

    let mut actions = Vec::new();
    for (size, target) in strategy_target_counts(config) {
        let current = state_count_for_size(state, size);
        if current < target {
            actions.push(PlannedAction {
                size,
                repeat: target - current,
                side: "sell".to_string(),
                pair: pair.clone(),
                expiry_unit: "minutes".to_string(),
                expiry_value: expiry_minutes,
                cancel_after_create: true,
                reason: "below_target".to_string(),
                target_spread_bps: config.target_spread_bps,
            });
        }
    }
    actions
}

pub fn evaluate_two_sided_market_actions(
    buy_state: &MarketState,
    sell_state: &MarketState,
    buy_config: &StrategyConfig,
    sell_config: &StrategyConfig,
) -> Vec<PlannedAction> {
    let mut actions = Vec::new();
    for (side, state, config) in [
        ("buy", buy_state, buy_config),
        ("sell", sell_state, sell_config),
    ] {
        for mut action in evaluate_market(state, config) {
            action.side = side.to_string();
            actions.push(action);
        }
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn sample_state() -> MarketState {
        MarketState {
            ones: 3,
            tens: 1,
            hundreds: 0,
            xch_price_usd: Some(32.0),
            bucket_counts_by_size: None,
        }
    }

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
    fn evaluate_market_returns_no_actions_when_targets_met() {
        let state = MarketState {
            ones: 5,
            tens: 2,
            hundreds: 1,
            xch_price_usd: Some(32.0),
            bucket_counts_by_size: None,
        };
        let config = StrategyConfig {
            pair: "xch".to_string(),
            ..sample_config()
        };
        assert!(evaluate_market(&state, &config).is_empty());
    }

    #[test]
    fn evaluate_market_plans_missing_sizes_in_order() {
        let actions = evaluate_market(&sample_state(), &sample_config());
        assert_eq!(
            actions,
            vec![
                PlannedAction {
                    size: 1,
                    repeat: 2,
                    pair: "xch".to_string(),
                    expiry_unit: "minutes".to_string(),
                    expiry_value: 10,
                    cancel_after_create: true,
                    reason: "below_target".to_string(),
                    target_spread_bps: None,
                    side: "sell".to_string(),
                },
                PlannedAction {
                    size: 10,
                    repeat: 1,
                    pair: "xch".to_string(),
                    expiry_unit: "minutes".to_string(),
                    expiry_value: 10,
                    cancel_after_create: true,
                    reason: "below_target".to_string(),
                    target_spread_bps: None,
                    side: "sell".to_string(),
                },
                PlannedAction {
                    size: 100,
                    repeat: 1,
                    pair: "xch".to_string(),
                    expiry_unit: "minutes".to_string(),
                    expiry_value: 10,
                    cancel_after_create: true,
                    reason: "below_target".to_string(),
                    target_spread_bps: None,
                    side: "sell".to_string(),
                },
            ]
        );
    }

    #[test]
    fn evaluate_market_xch_requires_price_before_planning() {
        let state = MarketState {
            xch_price_usd: None,
            ..sample_state()
        };
        assert!(evaluate_market(&state, &sample_config()).is_empty());
    }

    #[test]
    fn evaluate_two_sided_market_actions_assigns_side_labels() {
        let state = MarketState {
            ones: 0,
            tens: 0,
            hundreds: 0,
            xch_price_usd: None,
            bucket_counts_by_size: Some(BTreeMap::from([(10, 0)])),
        };
        let config = StrategyConfig {
            pair: "usdc".to_string(),
            target_counts_by_size: Some(BTreeMap::from([(10, 1)])),
            ..sample_config()
        };
        let actions = evaluate_two_sided_market_actions(&state, &state, &config, &config);
        assert_eq!(actions.len(), 2);
        assert!(actions.iter().any(|action| action.side == "buy"));
        assert!(actions.iter().any(|action| action.side == "sell"));
    }

    #[test]
    fn evaluate_market_respects_dynamic_target_sizes() {
        let state = MarketState {
            ones: 5,
            tens: 2,
            hundreds: 0,
            xch_price_usd: Some(30.0),
            bucket_counts_by_size: Some(BTreeMap::from([(1, 5), (10, 2), (50, 0)])),
        };
        let config = StrategyConfig {
            target_counts_by_size: Some(BTreeMap::from([(1, 5), (10, 2), (50, 1)])),
            ..sample_config()
        };
        let actions = evaluate_market(&state, &config);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].size, 50);
        assert_eq!(actions[0].repeat, 1);
    }
}
