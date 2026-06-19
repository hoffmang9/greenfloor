use std::collections::HashMap;
use std::path::Path;

use serde_json::{json, Value};

use super::yaml_fields::{
    config_err, optional_bool_value, optional_f64, optional_i64, optional_str,
    optional_trimmed_string,
};
use crate::config::markets_validate::{
    canonicalize_asset_unit_mojo_multiplier, pop_cancel_move_threshold_bps,
    validate_strategy_pricing,
};
use crate::config::program::is_testnet_network;
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
    pub pricing: Value,
    pub cancel_move_threshold_bps: Option<i64>,
    pub ladders: HashMap<String, Vec<LadderEntry>>,
}

#[derive(Debug, Clone)]
pub struct MarketsConfig {
    pub markets: Vec<MarketConfig>,
}

/// Load markets config.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn load_markets_config(path: &Path) -> SignerResult<MarketsConfig> {
    load_markets_config_with_overlay(path, None)
}

/// Load markets config with overlay.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn load_markets_config_with_overlay(
    base_path: &Path,
    overlay_path: Option<&Path>,
) -> SignerResult<MarketsConfig> {
    let mut raw = read_yaml_mapping(base_path)?;
    validate_base_markets_no_testnet_receive_addresses(base_path, &raw)?;
    if let Some(overlay) = overlay_path {
        if overlay.exists() {
            let overlay_raw = read_yaml_mapping(overlay)?;
            let base_markets = raw
                .get("markets")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let overlay_markets = overlay_raw
                .get("markets")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut merged = base_markets;
            merged.extend(overlay_markets);
            raw["markets"] = Value::Array(merged);
        }
    }
    parse_markets_config(&raw)
}

fn validate_base_markets_no_testnet_receive_addresses(
    path: &Path,
    raw: &Value,
) -> SignerResult<()> {
    let Some(rows) = raw.get("markets").and_then(Value::as_array) else {
        return Ok(());
    };
    let mut bad_ids: Vec<String> = Vec::new();
    for row in rows {
        let Some(row) = row.as_object() else {
            continue;
        };
        let receive_address = row
            .get("receive_address")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if receive_address.starts_with("txch1") {
            let market_id = row
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("<unknown>")
                .trim()
                .to_string();
            bad_ids.push(market_id);
        }
    }
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

fn read_yaml_mapping(path: &Path) -> SignerResult<Value> {
    let raw = std::fs::read_to_string(path).map_err(|err| {
        SignerError::Other(format!(
            "failed to read markets config {}: {err}",
            path.display()
        ))
    })?;
    serde_yaml::from_str(&raw).map_err(|err| {
        SignerError::Other(format!(
            "failed to parse markets config {}: {err}",
            path.display()
        ))
    })
}

/// Parse markets config.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn parse_markets_config(raw: &Value) -> SignerResult<MarketsConfig> {
    let root = raw
        .as_object()
        .ok_or_else(|| config_err("markets config root must be a mapping"))?;
    let empty: &[Value] = &[];
    let rows = root
        .get("markets")
        .and_then(Value::as_array)
        .map_or(empty, |rows| rows.as_slice());
    let mut markets = Vec::with_capacity(rows.len());
    for row in rows {
        let row = row
            .as_object()
            .ok_or_else(|| config_err("markets entries must be mappings"))?;
        markets.push(parse_market_row(row)?);
    }
    Ok(MarketsConfig { markets })
}

fn parse_market_row(row: &serde_json::Map<String, Value>) -> SignerResult<MarketConfig> {
    let market_id = optional_trimmed_string(row.get("id"))
        .ok_or_else(|| SignerError::Other("market id is required".to_string()))?;

    let mut ladders: HashMap<String, Vec<LadderEntry>> = HashMap::default();
    if let Some(ladder_map) = row.get("ladders").and_then(Value::as_object) {
        for (side, entries) in ladder_map {
            let Some(entries) = entries.as_array() else {
                return Err(config_err(format!(
                    "market {market_id}: ladders.{side} must be a list"
                )));
            };
            let parsed_entries = entries
                .iter()
                .map(|entry| {
                    let entry = entry.as_object().ok_or_else(|| {
                        config_err(format!(
                            "market {market_id}: ladders.{side} entries must be mappings"
                        ))
                    })?;
                    Ok(LadderEntry {
                        size_base_units: optional_i64(entry, "size_base_units", 0)?,
                        target_count: optional_i64(entry, "target_count", 0)?,
                        split_buffer_count: optional_i64(entry, "split_buffer_count", 0)?,
                        combine_when_excess_factor: optional_f64(
                            entry,
                            "combine_when_excess_factor",
                            2.0,
                        )?,
                    })
                })
                .collect::<SignerResult<Vec<_>>>()?;
            ladders.insert(side.clone(), parsed_entries);
        }
    }

    let base_asset = optional_str(row, "base_asset", "");
    let quote_asset = optional_str(row, "quote_asset", "");
    let quote_asset_type = optional_str(row, "quote_asset_type", "").to_ascii_lowercase();
    let mut pricing = row.get("pricing").cloned().unwrap_or_else(|| json!({}));
    let base_multiplier = canonicalize_asset_unit_mojo_multiplier(
        &base_asset,
        pricing.get("base_unit_mojo_multiplier"),
        "base_unit_mojo_multiplier",
        &market_id,
    )?;
    let quote_multiplier = canonicalize_asset_unit_mojo_multiplier(
        &quote_asset,
        pricing.get("quote_unit_mojo_multiplier"),
        "quote_unit_mojo_multiplier",
        &market_id,
    )?;
    if let Some(pricing_obj) = pricing.as_object_mut() {
        pricing_obj.insert(
            "base_unit_mojo_multiplier".to_string(),
            json!(base_multiplier),
        );
        pricing_obj.insert(
            "quote_unit_mojo_multiplier".to_string(),
            json!(quote_multiplier),
        );
    }
    validate_strategy_pricing(&pricing, &market_id, &quote_asset_type)?;
    let cancel_move_threshold_bps = pop_cancel_move_threshold_bps(&mut pricing)?;

    Ok(MarketConfig {
        market_id,
        enabled: optional_bool_value(row.get("enabled"), false),
        base_asset,
        base_symbol: optional_str(row, "base_symbol", ""),
        quote_asset,
        quote_asset_type,
        receive_address: optional_str(row, "receive_address", ""),
        signer_key_id: optional_str(row, "signer_key_id", ""),
        mode: optional_str(row, "mode", "sell_only").to_ascii_lowercase(),
        pricing,
        cancel_move_threshold_bps,
        ladders,
    })
}

pub fn cancel_policy_stable_vs_unstable(pricing: &Value) -> bool {
    pricing
        .get("cancel_policy_stable_vs_unstable")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Resolve market for build.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn resolve_market_for_build(
    markets: &MarketsConfig,
    market_id: Option<&str>,
    pair: Option<&str>,
    network: &str,
) -> SignerResult<MarketConfig> {
    let has_market_id = market_id
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    let has_pair = pair.map(str::trim).is_some_and(|value| !value.is_empty());
    if has_market_id == has_pair {
        return Err(SignerError::Other(
            "provide exactly one of --market-id or --pair".to_string(),
        ));
    }
    if let Some(market_id) = market_id.map(str::trim).filter(|value| !value.is_empty()) {
        return markets
            .markets
            .iter()
            .find(|market| market.market_id == market_id)
            .cloned()
            .ok_or_else(|| SignerError::Other(format!("market_id not found: {market_id}")));
    }
    let pair = pair.expect("pair checked above").trim();
    let sep = if pair.contains(':') {
        ':'
    } else if pair.contains('/') {
        '/'
    } else {
        return Err(SignerError::Other(
            "pair must be in base:quote or base/quote format".to_string(),
        ));
    };
    let mut parts = pair.splitn(2, sep);
    let base_raw = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| SignerError::Other("pair base must be non-empty".to_string()))?
        .to_ascii_lowercase();
    let quote_raw = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| SignerError::Other("pair quote must be non-empty".to_string()))?
        .to_ascii_lowercase();

    let mut candidates = Vec::new();
    for market in &markets.markets {
        if !market.enabled {
            continue;
        }
        let base_matches = [
            market.base_asset.trim().to_ascii_lowercase(),
            market.base_symbol.trim().to_ascii_lowercase(),
        ];
        let quote_match = market.quote_asset.trim().to_ascii_lowercase();
        let mut quote_matches = vec![quote_match.clone()];
        if is_testnet_network(network) {
            if quote_match == "xch" {
                quote_matches.push("txch".to_string());
            } else if quote_match == "txch" {
                quote_matches.push("xch".to_string());
            }
        }
        if base_matches.iter().any(|value| value == &base_raw)
            && quote_matches.iter().any(|value| value == &quote_raw)
        {
            candidates.push(market.clone());
        }
    }
    if candidates.is_empty() {
        return Err(SignerError::Other(format!(
            "no enabled market found for pair: {pair}"
        )));
    }
    if candidates.len() > 1 {
        let ids: Vec<_> = candidates
            .iter()
            .map(|market| market.market_id.as_str())
            .collect();
        return Err(SignerError::Other(format!(
            "pair is ambiguous; use --market-id (candidates: {})",
            ids.join(", ")
        )));
    }
    Ok(candidates.remove(0))
}
