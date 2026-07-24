use std::collections::HashMap;

use serde_json::{Map, Value};

use super::pricing::parse_market_pricing;
use super::{LadderEntry, MarketConfig, MarketsConfig};
use crate::config::yaml_fields::{
    config_err, optional_bool_value, optional_f64, optional_i64, optional_str,
    optional_trimmed_string,
};
use crate::error::{SignerError, SignerResult};

/// Parse markets config from a JSON/YAML value tree.
///
/// # Errors
///
/// Returns an error if the document shape or market fields are invalid.
pub fn parse_markets_config(raw: &Value) -> SignerResult<MarketsConfig> {
    let root = raw
        .as_object()
        .ok_or_else(|| config_err("markets config root must be a mapping"))?;
    let empty: &[Value] = &[];
    let rows = root
        .get("markets")
        .and_then(Value::as_array)
        .map_or(empty, Vec::as_slice);
    let markets = rows
        .iter()
        .map(|row| {
            let row = row
                .as_object()
                .ok_or_else(|| config_err("markets entries must be mappings"))?;
            parse_market_row(row)
        })
        .collect::<SignerResult<_>>()?;
    Ok(MarketsConfig { markets })
}

fn parse_market_row(row: &Map<String, Value>) -> SignerResult<MarketConfig> {
    let market_id = optional_trimmed_string(row.get("id"))
        .ok_or_else(|| SignerError::Other("market id is required".to_string()))?;
    let ladders = parse_ladders(&market_id, row.get("ladders"))?;
    let base_asset = optional_str(row, "base_asset", "");
    let quote_asset = optional_str(row, "quote_asset", "");
    let quote_asset_type = optional_str(row, "quote_asset_type", "").to_ascii_lowercase();
    let parsed = parse_market_pricing(
        row.get("pricing"),
        &base_asset,
        &quote_asset,
        &quote_asset_type,
        &market_id,
    )?;

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
        pricing: parsed.pricing,
        cancel_move_threshold_bps: parsed.cancel_move_threshold_bps,
        ladders,
    })
}

fn parse_ladders(
    market_id: &str,
    ladders_raw: Option<&Value>,
) -> SignerResult<HashMap<String, Vec<LadderEntry>>> {
    let Some(ladder_map) = ladders_raw.and_then(Value::as_object) else {
        return Ok(HashMap::new());
    };
    let mut ladders = HashMap::with_capacity(ladder_map.len());
    for (side, entries) in ladder_map {
        let entries = entries.as_array().ok_or_else(|| {
            config_err(format!("market {market_id}: ladders.{side} must be a list"))
        })?;
        let parsed = entries
            .iter()
            .map(|entry| parse_ladder_entry(market_id, side, entry))
            .collect::<SignerResult<Vec<_>>>()?;
        ladders.insert(side.clone(), parsed);
    }
    Ok(ladders)
}

fn parse_ladder_entry(market_id: &str, side: &str, entry: &Value) -> SignerResult<LadderEntry> {
    let entry = entry.as_object().ok_or_else(|| {
        config_err(format!(
            "market {market_id}: ladders.{side} entries must be mappings"
        ))
    })?;
    Ok(LadderEntry {
        size_base_units: optional_i64(entry, "size_base_units", 0)?,
        target_count: optional_i64(entry, "target_count", 0)?,
        split_buffer_count: optional_i64(entry, "split_buffer_count", 0)?,
        combine_when_excess_factor: optional_f64(entry, "combine_when_excess_factor", 2.0)?,
    })
}
