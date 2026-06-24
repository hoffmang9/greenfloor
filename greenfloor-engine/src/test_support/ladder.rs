//! Shared sell-ladder fixtures for unit tests.

use std::collections::HashMap;

use crate::config::{LadderEntry, MarketConfig};

use super::market_config::sample_market;

#[must_use]
pub fn sell_ladder_entries(size_base_units: i64, target_count: i64) -> Vec<LadderEntry> {
    vec![LadderEntry {
        size_base_units,
        target_count,
        split_buffer_count: 1,
        combine_when_excess_factor: 2.0,
    }]
}

#[must_use]
pub fn market_with_sell_ladder(
    receive_address: impl AsRef<str>,
    size_base_units: i64,
    target_count: i64,
) -> MarketConfig {
    let mut market = sample_market(receive_address);
    market.ladders.insert(
        "sell".to_string(),
        sell_ladder_entries(size_base_units, target_count),
    );
    market
}

#[must_use]
pub fn market_with_side_ladder(
    receive_address: impl AsRef<str>,
    side: &str,
    size_base_units: i64,
    target_count: i64,
) -> MarketConfig {
    let mut market = sample_market(receive_address);
    market.ladders.insert(
        side.to_string(),
        sell_ladder_entries(size_base_units, target_count),
    );
    market
}

#[must_use]
pub fn empty_ladders_market(receive_address: impl AsRef<str>) -> MarketConfig {
    let mut market = sample_market(receive_address);
    market.ladders = HashMap::new();
    market
}

#[must_use]
pub fn eco181_sell_ladder_entries() -> Vec<LadderEntry> {
    vec![
        LadderEntry {
            size_base_units: 1,
            target_count: 5,
            split_buffer_count: 1,
            combine_when_excess_factor: 2.0,
        },
        LadderEntry {
            size_base_units: 10,
            target_count: 2,
            split_buffer_count: 1,
            combine_when_excess_factor: 2.0,
        },
        LadderEntry {
            size_base_units: 100,
            target_count: 1,
            split_buffer_count: 0,
            combine_when_excess_factor: 2.0,
        },
    ]
}

#[must_use]
pub fn market_with_eco181_sell_ladder(receive_address: impl AsRef<str>) -> MarketConfig {
    let mut market = sample_market(receive_address);
    market
        .ladders
        .insert("sell".to_string(), eco181_sell_ladder_entries());
    market
}
