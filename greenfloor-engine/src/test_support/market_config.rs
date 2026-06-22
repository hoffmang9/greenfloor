//! Minimal market config fixtures for unit tests.

use std::collections::HashMap;

use serde_json::json;

use crate::config::MarketConfig;

#[must_use]
pub fn sample_market(receive_address: impl AsRef<str>) -> MarketConfig {
    MarketConfig {
        market_id: "m1".to_string(),
        enabled: true,
        base_asset: "xch".to_string(),
        base_symbol: "XCH".to_string(),
        quote_asset: "xch".to_string(),
        quote_asset_type: "unstable".to_string(),
        receive_address: receive_address.as_ref().to_string(),
        signer_key_id: "key-1".to_string(),
        mode: "sell_only".to_string(),
        pricing: json!({}),
        cancel_move_threshold_bps: None,
        ladders: HashMap::default(),
    }
}
