use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::config::program::is_testnet_network;
use crate::error::{SignerError, SignerResult};

#[derive(Debug, Clone)]
pub struct LadderEntry {
    pub size_base_units: i64,
    pub target_count: i64,
    pub split_buffer_count: i64,
}

#[derive(Debug, Clone)]
pub struct MarketConfig {
    pub market_id: String,
    pub enabled: bool,
    pub base_asset: String,
    pub base_symbol: String,
    pub quote_asset: String,
    pub receive_address: String,
    pub pricing: Value,
    pub ladders: HashMap<String, Vec<LadderEntry>>,
}

#[derive(Debug, Clone)]
pub struct MarketsConfig {
    pub markets: Vec<MarketConfig>,
}

#[derive(Debug, Deserialize)]
struct MarketsYaml {
    markets: Option<Vec<MarketYaml>>,
}

#[derive(Debug, Deserialize)]
struct MarketYaml {
    id: Option<String>,
    enabled: Option<bool>,
    base_asset: Option<String>,
    base_symbol: Option<String>,
    quote_asset: Option<String>,
    receive_address: Option<String>,
    pricing: Option<Value>,
    ladders: Option<HashMap<String, Vec<LadderEntryYaml>>>,
}

#[derive(Debug, Deserialize)]
struct LadderEntryYaml {
    size_base_units: Option<i64>,
    target_count: Option<i64>,
    split_buffer_count: Option<i64>,
}

pub fn load_markets_config(path: &Path) -> SignerResult<MarketsConfig> {
    load_markets_config_with_overlay(path, None)
}

pub fn load_markets_config_with_overlay(
    base_path: &Path,
    overlay_path: Option<&Path>,
) -> SignerResult<MarketsConfig> {
    let mut raw = read_yaml_mapping(base_path)?;
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

fn read_yaml_mapping(path: &Path) -> SignerResult<Value> {
    let raw = std::fs::read_to_string(path).map_err(|err| {
        SignerError::Other(format!("failed to read markets config {}: {err}", path.display()))
    })?;
    serde_yaml::from_str(&raw).map_err(|err| {
        SignerError::Other(format!("failed to parse markets config {}: {err}", path.display()))
    })
}

pub fn parse_markets_config(raw: &Value) -> SignerResult<MarketsConfig> {
    let parsed: MarketsYaml = serde_json::from_value(raw.clone()).map_err(|err| {
        SignerError::Other(format!("invalid markets config shape: {err}"))
    })?;
    let rows = parsed.markets.unwrap_or_default();
    let mut markets = Vec::with_capacity(rows.len());
    for row in rows {
        let market_id = row
            .id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| SignerError::Other("market id is required".to_string()))?;
        let mut ladders: HashMap<String, Vec<LadderEntry>> = HashMap::new();
        if let Some(raw_ladders) = row.ladders {
            for (side, entries) in raw_ladders {
                let parsed_entries = entries
                    .into_iter()
                    .map(|entry| LadderEntry {
                        size_base_units: entry.size_base_units.unwrap_or(0),
                        target_count: entry.target_count.unwrap_or(0),
                        split_buffer_count: entry.split_buffer_count.unwrap_or(0),
                    })
                    .collect();
                ladders.insert(side, parsed_entries);
            }
        }
        markets.push(MarketConfig {
            market_id,
            enabled: row.enabled.unwrap_or(false),
            base_asset: row
                .base_asset
                .unwrap_or_default()
                .trim()
                .to_string(),
            base_symbol: row
                .base_symbol
                .unwrap_or_default()
                .trim()
                .to_string(),
            quote_asset: row
                .quote_asset
                .unwrap_or_default()
                .trim()
                .to_string(),
            receive_address: row
                .receive_address
                .unwrap_or_default()
                .trim()
                .to_string(),
            pricing: row.pricing.unwrap_or_else(|| json!({})),
            ladders,
        });
    }
    Ok(MarketsConfig { markets })
}

pub fn resolve_market_for_build(
    markets: &MarketsConfig,
    market_id: Option<&str>,
    pair: Option<&str>,
    network: &str,
) -> SignerResult<MarketConfig> {
    let has_market_id = market_id.map(str::trim).is_some_and(|value| !value.is_empty());
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_markets() -> MarketsConfig {
        parse_markets_config(&json!({
            "markets": [{
                "id": "m1",
                "enabled": true,
                "base_asset": "a1",
                "base_symbol": "A1",
                "quote_asset": "xch",
                "receive_address": "xch1test",
                "pricing": {"min_price_quote_per_base": 0.0031}
            }]
        }))
        .expect("markets")
    }

    #[test]
    fn resolves_market_by_id() {
        let markets = sample_markets();
        let market = resolve_market_for_build(&markets, Some("m1"), None, "mainnet").expect("market");
        assert_eq!(market.market_id, "m1");
    }

    #[test]
    fn resolves_market_by_pair() {
        let markets = sample_markets();
        let market =
            resolve_market_for_build(&markets, None, Some("A1:xch"), "mainnet").expect("market");
        assert_eq!(market.market_id, "m1");
    }
}
