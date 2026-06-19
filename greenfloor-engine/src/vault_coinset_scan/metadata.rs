use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use serde_json::Value as JsonValue;

use crate::config::{load_markets_config_with_overlay, MarketConfig};
use crate::error::SignerResult;
use crate::hex::{is_hex_id, normalize_hex_id};
use crate::manager_cli::load_cats_catalog;

pub fn normalize_label(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

pub fn parse_csv_values(values: &[String]) -> Vec<String> {
    let mut parsed = Vec::new();
    for value in values {
        for segment in value.split(',') {
            let trimmed = segment.trim();
            if !trimmed.is_empty() {
                parsed.push(trimmed.to_string());
            }
        }
    }
    parsed
}

pub fn resolve_requested_cat_ids(
    cat_ids: &[String],
    cat_tickers: &[String],
    ticker_to_asset_ids: &HashMap<String, HashSet<String>>,
) -> (HashSet<String>, Vec<String>) {
    let mut resolved = HashSet::new();
    for raw_id in cat_ids {
        let clean = normalize_hex_id(raw_id);
        if !clean.is_empty() {
            resolved.insert(clean);
        }
    }
    let mut unresolved_tickers = Vec::new();
    for ticker in cat_tickers {
        let key = normalize_label(ticker);
        let matches = ticker_to_asset_ids.get(&key);
        if matches.is_none_or(HashSet::is_empty) {
            unresolved_tickers.push(ticker.trim().to_string());
            continue;
        }
        if let Some(matches) = matches {
            resolved.extend(matches.iter().cloned());
        }
    }
    (resolved, unresolved_tickers)
}

fn add_mapping(
    ticker_to_asset_ids: &mut HashMap<String, HashSet<String>>,
    asset_id_to_symbols: &mut HashMap<String, BTreeSet<String>>,
    ticker: &str,
    asset_id: &str,
) {
    let clean_asset_id = normalize_hex_id(asset_id);
    let clean_ticker = normalize_label(ticker);
    if clean_asset_id.is_empty() || clean_ticker.is_empty() {
        return;
    }
    ticker_to_asset_ids
        .entry(clean_ticker)
        .or_default()
        .insert(clean_asset_id.clone());
    asset_id_to_symbols
        .entry(clean_asset_id)
        .or_default()
        .insert(ticker.trim().to_string());
}

fn add_cat_row_mappings(
    ticker_to_asset_ids: &mut HashMap<String, HashSet<String>>,
    asset_id_to_symbols: &mut HashMap<String, BTreeSet<String>>,
    row: &JsonValue,
) {
    let Some(asset_id) = row.get("asset_id").and_then(JsonValue::as_str) else {
        return;
    };
    if let Some(base_symbol) = row.get("base_symbol").and_then(JsonValue::as_str) {
        add_mapping(
            ticker_to_asset_ids,
            asset_id_to_symbols,
            base_symbol,
            asset_id,
        );
    }
    if let Some(name) = row.get("name").and_then(JsonValue::as_str) {
        add_mapping(ticker_to_asset_ids, asset_id_to_symbols, name, asset_id);
    }
    if let Some(aliases) = row.get("aliases").and_then(JsonValue::as_array) {
        for alias in aliases {
            if let Some(alias) = alias.as_str() {
                add_mapping(ticker_to_asset_ids, asset_id_to_symbols, alias, asset_id);
            }
        }
    }
}

fn add_market_row_mappings(
    ticker_to_asset_ids: &mut HashMap<String, HashSet<String>>,
    asset_id_to_symbols: &mut HashMap<String, BTreeSet<String>>,
    market: &MarketConfig,
) {
    add_mapping(
        ticker_to_asset_ids,
        asset_id_to_symbols,
        &market.base_symbol,
        &market.base_asset,
    );
    let quote_asset = market.quote_asset.trim();
    if is_hex_id(quote_asset) {
        add_mapping(
            ticker_to_asset_ids,
            asset_id_to_symbols,
            quote_asset,
            quote_asset,
        );
    }
}

pub type CatMetadataIndexes = (
    HashMap<String, HashSet<String>>,
    BTreeMap<String, Vec<String>>,
);

pub fn load_cat_metadata_indexes(
    cats_config: &Path,
    markets_config: &Path,
    testnet_markets_config: Option<&Path>,
) -> SignerResult<CatMetadataIndexes> {
    let mut ticker_to_asset_ids: HashMap<String, HashSet<String>> = HashMap::new();
    let mut asset_id_to_symbols: HashMap<String, BTreeSet<String>> = HashMap::new();

    if cats_config.exists() {
        let catalog = load_cats_catalog(cats_config)?;
        for row in &catalog {
            add_cat_row_mappings(&mut ticker_to_asset_ids, &mut asset_id_to_symbols, row);
        }
    }

    if markets_config.exists() {
        let markets =
            load_markets_config_with_overlay(markets_config, testnet_markets_config)?.markets;
        for market in &markets {
            add_market_row_mappings(&mut ticker_to_asset_ids, &mut asset_id_to_symbols, market);
        }
    }

    let frozen_asset_to_symbols = asset_id_to_symbols
        .into_iter()
        .map(|(asset_id, symbols)| (asset_id, symbols.into_iter().collect()))
        .collect();
    Ok((ticker_to_asset_ids, frozen_asset_to_symbols))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_label_strips_non_alnum() {
        assert_eq!(normalize_label(" wUSDC.b "), "wusdcb");
    }

    #[test]
    fn parse_csv_values_splits_and_trims() {
        assert_eq!(
            parse_csv_values(&["a,b".to_string(), " c ".to_string()]),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn resolve_requested_cat_ids_resolves_tickers() {
        let mut ticker_map = HashMap::new();
        ticker_map.insert("wusdcb".to_string(), HashSet::from(["aa".repeat(64)]));
        let (resolved, unresolved) =
            resolve_requested_cat_ids(&[], &["wUSDC.b".to_string()], &ticker_map);
        assert!(unresolved.is_empty());
        assert_eq!(resolved.len(), 1);
        assert!(resolved.contains(&"aa".repeat(64)));
    }

    #[test]
    fn resolve_requested_cat_ids_reports_unknown_tickers() {
        let (resolved, unresolved) =
            resolve_requested_cat_ids(&[], &["NOPE".to_string()], &HashMap::new());
        assert!(resolved.is_empty());
        assert_eq!(unresolved, vec!["NOPE".to_string()]);
    }
}
