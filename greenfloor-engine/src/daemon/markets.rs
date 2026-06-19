use std::collections::HashSet;

use crate::config::MarketsConfig;

/// Enabled market ids in config order, deduplicated.
#[must_use]
pub fn enabled_market_ids(markets: &MarketsConfig) -> Vec<String> {
    let mut enabled = Vec::new();
    let mut seen = HashSet::new();
    for market in &markets.markets {
        if !market.enabled {
            continue;
        }
        let market_id = market.market_id.trim();
        if market_id.is_empty() || !seen.insert(market_id.to_string()) {
            continue;
        }
        enabled.push(market_id.to_string());
    }
    enabled
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MarketConfig, MarketsConfig};
    use serde_json::json;
    use std::collections::HashMap;

    fn sample_market(market_id: &str, enabled: bool) -> MarketConfig {
        MarketConfig {
            market_id: market_id.to_string(),
            enabled,
            base_asset: "asset1".to_string(),
            base_symbol: "AS1".to_string(),
            quote_asset: "xch".to_string(),
            quote_asset_type: "unstable".to_string(),
            receive_address: "xch1test".to_string(),
            signer_key_id: "key-1".to_string(),
            mode: "sell_only".to_string(),
            pricing: json!({}),
            cancel_move_threshold_bps: None,
            ladders: HashMap::new(),
        }
    }

    #[test]
    fn enabled_market_ids_skips_disabled_and_dedupes() {
        let markets = MarketsConfig {
            markets: vec![
                sample_market("m1", true),
                sample_market("m1", true),
                sample_market("m2", false),
                sample_market("m3", true),
            ],
        };
        assert_eq!(enabled_market_ids(&markets), vec!["m1", "m3"]);
    }
}
