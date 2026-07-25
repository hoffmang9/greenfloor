//! Markets config: types, YAML load/parse, and market selection.

mod parse;
mod pricing;
mod resolve;

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

pub use parse::parse_markets_config;
pub use pricing::MarketPricing;
pub use resolve::{
    ensure_market_receive_address_for_network, receive_address_matches_operator_network,
    resolve_market_for_build, resolve_operator_market, OperatorMarketCommand,
};

use crate::config::yaml_file::read_yaml_file_labeled;
use crate::error::{SignerError, SignerResult};

#[derive(Debug, Clone)]
pub struct LadderEntry {
    pub size_base_units: i64,
    pub target_count: i64,
    pub split_buffer_count: i64,
    pub combine_when_excess_factor: f64,
}

#[derive(Debug, Clone)]
pub struct MarketConfig {
    pub market_id: String,
    pub enabled: bool,
    pub base_asset: String,
    pub base_symbol: String,
    pub quote_asset: String,
    pub quote_asset_type: String,
    pub receive_address: String,
    pub signer_key_id: String,
    pub mode: String,
    pub pricing: MarketPricing,
    pub cancel_move_threshold_bps: Option<i64>,
    pub ladders: HashMap<String, Vec<LadderEntry>>,
}

#[derive(Debug, Clone)]
pub struct MarketsConfig {
    pub markets: Vec<MarketConfig>,
}

/// Load markets config (base file only).
///
/// # Errors
///
/// Returns an error if the YAML cannot be read or parsed.
pub fn load_markets_config(path: &Path) -> SignerResult<MarketsConfig> {
    load_markets_config_with_overlay(path, None)
}

/// Load markets config, appending overlay markets when the overlay path exists.
///
/// # Errors
///
/// Returns an error if a YAML file cannot be read/parsed, or if the base file
/// contains testnet `receive_address` values.
pub fn load_markets_config_with_overlay(
    base_path: &Path,
    overlay_path: Option<&Path>,
) -> SignerResult<MarketsConfig> {
    let mut raw = read_yaml_file_labeled(base_path, "markets config")?;
    reject_testnet_receive_addresses_in_base(base_path, &raw)?;
    if let Some(overlay) = overlay_path.filter(|path| path.exists()) {
        let overlay_raw = read_yaml_file_labeled(overlay, "markets config")?;
        let mut merged = market_rows(&raw);
        merged.extend(market_rows(&overlay_raw));
        raw["markets"] = Value::Array(merged);
    }
    parse_markets_config(&raw)
}

fn market_rows(raw: &Value) -> Vec<Value> {
    raw.get("markets")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn reject_testnet_receive_addresses_in_base(path: &Path, raw: &Value) -> SignerResult<()> {
    let Some(rows) = raw.get("markets").and_then(Value::as_array) else {
        return Ok(());
    };
    let bad_ids: Vec<String> = rows
        .iter()
        .filter_map(Value::as_object)
        .filter(|row| {
            let addr = row
                .get("receive_address")
                .and_then(Value::as_str)
                .unwrap_or_default();
            receive_address_matches_operator_network(addr, "testnet11")
        })
        .map(|row| {
            row.get("id")
                .and_then(Value::as_str)
                .unwrap_or("<unknown>")
                .trim()
                .to_string()
        })
        .collect();
    if bad_ids.is_empty() {
        return Ok(());
    }
    Err(SignerError::Other(format!(
        "testnet receive_address entries found in base markets config {}; \
         move these markets to testnet-markets.yaml (market_ids={})",
        path.display(),
        bad_ids.join(",")
    )))
}
