use std::collections::BTreeMap;

use serde_json::Value;

use crate::config::{MarketConfig, resolve_quote_asset_for_offer};
use crate::cycle::{
    filter_planned_actions_with_positive_repeat, is_two_sided_market_mode,
    one_sided_offer_counts_by_side, plan_reseed_actions_from_gap, resolve_tracked_sizes,
    MarketState, StrategyConfig,
};
use crate::daemon::coinset_tx::dexie_offer_status;

pub fn strategy_config_from_market(market: &MarketConfig, network: &str) -> StrategyConfig {
    let sell_ladder = market.ladders.get("sell").cloned().unwrap_or_default();
    let mut targets_by_size: BTreeMap<i64, i64> = BTreeMap::new();
    for entry in &sell_ladder {
        if entry.size_base_units > 0 {
            targets_by_size.insert(entry.size_base_units, entry.target_count.max(0));
        }
    }
    let normalized = normalize_target_counts(&targets_by_size, Some(&default_target_counts()));
    let pricing = &market.pricing;
    StrategyConfig {
        pair: normalize_strategy_pair(&market.quote_asset, network),
        ones_target: *normalized.get(&1).unwrap_or(&0),
        tens_target: *normalized.get(&10).unwrap_or(&0),
        hundreds_target: *normalized.get(&100).unwrap_or(&0),
        target_spread_bps: pricing_int(pricing, "strategy_target_spread_bps"),
        min_xch_price_usd: pricing_float(pricing, "strategy_min_xch_price_usd"),
        max_xch_price_usd: pricing_float(pricing, "strategy_max_xch_price_usd"),
        offer_expiry_minutes: pricing_int(pricing, "strategy_offer_expiry_minutes"),
        target_counts_by_size: Some(normalized),
    }
}

pub fn strategy_state_from_bucket_counts(
    bucket_counts: &BTreeMap<i64, i64>,
    xch_price_usd: Option<f64>,
) -> MarketState {
    MarketState {
        ones: *bucket_counts.get(&1).unwrap_or(&0),
        tens: *bucket_counts.get(&10).unwrap_or(&0),
        hundreds: *bucket_counts.get(&100).unwrap_or(&0),
        xch_price_usd,
        bucket_counts_by_size: Some(bucket_counts.clone()),
    }
}

pub fn active_offer_counts_by_size(
    offers: &[Value],
    dexie_size_by_offer_id: &std::collections::HashMap<String, i64>,
) -> BTreeMap<i64, i64> {
    let mut counts: BTreeMap<i64, i64> = BTreeMap::new();
    for offer in offers {
        let Some(offer_id) = offer
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        if dexie_offer_status(offer).unwrap_or(-1) != 0 {
            continue;
        }
        let size = dexie_size_by_offer_id.get(offer_id).copied().unwrap_or(0);
        if size > 0 {
            *counts.entry(size).or_insert(0) += 1;
        }
    }
    counts
}

pub fn evaluate_strategy_actions(
    market: &MarketConfig,
    network: &str,
    active_counts: &BTreeMap<i64, i64>,
    xch_price_usd: Option<f64>,
) -> Vec<crate::cycle::PlannedAction> {
    let config = strategy_config_from_market(market, network);
    let market_mode = market_mode_label(market);
    if is_two_sided_market_mode(&market_mode) {
        let tracked: Vec<i64> = resolve_tracked_sizes(
            &ladder_sizes(market),
            &target_sizes_from_config(&config),
        );
        let (buy_side, sell_side) =
            one_sided_offer_counts_by_side(active_counts, &tracked);
        let buy_state = strategy_state_from_bucket_counts(&buy_side, xch_price_usd);
        let sell_state = strategy_state_from_bucket_counts(&sell_side, xch_price_usd);
        let buy_config = strategy_config_for_side(market, network, "buy");
        let sell_config = strategy_config_for_side(market, network, "sell");
        return crate::cycle::evaluate_two_sided_market_actions(
            &buy_state,
            &sell_state,
            &buy_config,
            &sell_config,
        );
    }
    let mut actions = crate::cycle::evaluate_market(
        &strategy_state_from_bucket_counts(active_counts, xch_price_usd),
        &config,
    );
    actions = filter_planned_actions_with_positive_repeat(&actions);
    let target_counts = config
        .target_counts_by_size
        .clone()
        .unwrap_or_default();
    let reseed = plan_reseed_actions_from_gap(
        &actions,
        active_counts,
        &target_counts,
        &config,
        xch_price_usd,
    );
    reseed.actions
}

fn strategy_config_for_side(market: &MarketConfig, network: &str, side: &str) -> StrategyConfig {
    let ladder = market.ladders.get(side).cloned().unwrap_or_default();
    let mut targets_by_size: BTreeMap<i64, i64> = BTreeMap::new();
    for entry in &ladder {
        if entry.size_base_units > 0 {
            targets_by_size.insert(entry.size_base_units, entry.target_count.max(0));
        }
    }
    let normalized = normalize_target_counts(&targets_by_size, None);
    let pricing = &market.pricing;
    StrategyConfig {
        pair: normalize_strategy_pair(&market.quote_asset, network),
        ones_target: *normalized.get(&1).unwrap_or(&0),
        tens_target: *normalized.get(&10).unwrap_or(&0),
        hundreds_target: *normalized.get(&100).unwrap_or(&0),
        target_spread_bps: None,
        min_xch_price_usd: None,
        max_xch_price_usd: None,
        offer_expiry_minutes: pricing_int(pricing, "strategy_offer_expiry_minutes"),
        target_counts_by_size: Some(normalized),
    }
}

fn normalize_strategy_pair(quote_asset: &str, network: &str) -> String {
    resolve_quote_asset_for_offer(quote_asset, network)
}

fn normalize_target_counts(
    raw: &BTreeMap<i64, i64>,
    defaults: Option<&BTreeMap<i64, i64>>,
) -> BTreeMap<i64, i64> {
    let mut out: BTreeMap<i64, i64> = raw
        .iter()
        .filter(|(size, _)| **size > 0)
        .map(|(size, target)| (*size, (*target).max(0)))
        .collect();
    if out.is_empty() {
        if let Some(defaults) = defaults {
            return defaults.clone();
        }
    }
    out
}

fn default_target_counts() -> BTreeMap<i64, i64> {
    BTreeMap::from([(1, 5), (10, 2), (100, 1)])
}

fn target_sizes_from_config(config: &StrategyConfig) -> Vec<i64> {
    if let Some(targets) = &config.target_counts_by_size {
        return targets.keys().copied().collect();
    }
    vec![1, 10, 100]
}

fn ladder_sizes(market: &MarketConfig) -> Vec<i64> {
    market
        .ladders
        .values()
        .flat_map(|entries| entries.iter().map(|entry| entry.size_base_units))
        .filter(|size| *size > 0)
        .collect()
}

fn market_mode_label(market: &MarketConfig) -> String {
    market.mode.trim().to_ascii_lowercase()
}

fn pricing_int(pricing: &Value, key: &str) -> Option<i64> {
    pricing.get(key).and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_u64().map(|raw| raw as i64))
    })
}

fn pricing_float(pricing: &Value, key: &str) -> Option<f64> {
    pricing.get(key).and_then(|value| value.as_f64())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LadderEntry;
    use serde_json::json;
    use std::collections::HashMap;

    fn sample_market() -> MarketConfig {
        let mut ladders = HashMap::new();
        ladders.insert(
            "sell".to_string(),
            vec![LadderEntry {
                size_base_units: 1,
                target_count: 1,
                split_buffer_count: 0,
                combine_when_excess_factor: 2.0,
            }],
        );
        MarketConfig {
            market_id: "m1".to_string(),
            enabled: true,
            base_asset: "asset1".to_string(),
            base_symbol: "AS1".to_string(),
            quote_asset: "xch".to_string(),
            quote_asset_type: "unstable".to_string(),
            receive_address: "xch1test".to_string(),
            signer_key_id: "key-main-1".to_string(),
            mode: "sell_only".to_string(),
            pricing: json!({"cancel_policy_stable_vs_unstable": true}),
            cancel_move_threshold_bps: None,
            ladders,
        }
    }

    #[test]
    fn strategy_config_reads_sell_ladder_targets() {
        let market = sample_market();
        let config = strategy_config_from_market(&market, "mainnet");
        assert_eq!(config.ones_target, 1);
        assert_eq!(config.pair, "xch");
    }

    #[test]
    fn active_offer_counts_only_open_offers() {
        let offers = vec![json!({"id": "o1", "status": 0}), json!({"id": "o2", "status": 3})];
        let sizes = HashMap::from([("o1".to_string(), 1_i64)]);
        let counts = active_offer_counts_by_size(&offers, &sizes);
        assert_eq!(counts.get(&1), Some(&1));
        assert_eq!(counts.get(&2), None);
    }
}
