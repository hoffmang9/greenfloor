//! Ticker→asset-id index from cats catalog and markets config.

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

pub type CatTickerIndex = (
    HashMap<String, HashSet<String>>,
    BTreeMap<String, Vec<String>>,
);

pub fn build_cat_ticker_index(
    cats_config: &Path,
    markets_config: &Path,
    testnet_markets_config: Option<&Path>,
) -> SignerResult<CatTickerIndex> {
    merge_ticker_index(cats_config, markets_config, testnet_markets_config, true)
}

/// Best-effort ticker index: skip unreadable config files instead of failing the whole load.
pub fn build_cat_ticker_index_lenient(
    cats_config: &Path,
    markets_config: &Path,
    testnet_markets_config: Option<&Path>,
) -> CatTickerIndex {
    merge_ticker_index(cats_config, markets_config, testnet_markets_config, false)
        .unwrap_or_else(|_| (HashMap::new(), BTreeMap::new()))
}

fn merge_ticker_index(
    cats_config: &Path,
    markets_config: &Path,
    testnet_markets_config: Option<&Path>,
    strict: bool,
) -> SignerResult<CatTickerIndex> {
    let mut ticker_to_asset_ids: HashMap<String, HashSet<String>> = HashMap::new();
    let mut asset_id_to_symbols: HashMap<String, BTreeSet<String>> = HashMap::new();

    merge_cats_catalog(
        cats_config,
        strict,
        &mut ticker_to_asset_ids,
        &mut asset_id_to_symbols,
    )?;
    merge_markets_config(
        markets_config,
        testnet_markets_config,
        strict,
        &mut ticker_to_asset_ids,
        &mut asset_id_to_symbols,
    )?;

    let asset_id_to_symbols = asset_id_to_symbols
        .into_iter()
        .map(|(asset_id, symbols)| (asset_id, symbols.into_iter().collect()))
        .collect();
    Ok((ticker_to_asset_ids, asset_id_to_symbols))
}

fn merge_cats_catalog(
    cats_config: &Path,
    strict: bool,
    ticker_to_asset_ids: &mut HashMap<String, HashSet<String>>,
    asset_id_to_symbols: &mut HashMap<String, BTreeSet<String>>,
) -> SignerResult<()> {
    if !cats_config.exists() {
        return Ok(());
    }
    match load_cats_catalog(cats_config) {
        Ok(catalog) => {
            for row in &catalog {
                add_cat_row_mappings(ticker_to_asset_ids, asset_id_to_symbols, row);
            }
        }
        Err(err) => {
            if strict {
                return Err(err);
            }
            tracing::warn!(
                path = %cats_config.display(),
                error = %err,
                "vault coinset scan: skipping unreadable cats catalog"
            );
        }
    }
    Ok(())
}

fn merge_markets_config(
    markets_config: &Path,
    testnet_markets_config: Option<&Path>,
    strict: bool,
    ticker_to_asset_ids: &mut HashMap<String, HashSet<String>>,
    asset_id_to_symbols: &mut HashMap<String, BTreeSet<String>>,
) -> SignerResult<()> {
    if !markets_config.exists() {
        return Ok(());
    }
    match load_markets_config_with_overlay(markets_config, testnet_markets_config) {
        Ok(cfg) => {
            for market in &cfg.markets {
                add_market_row_mappings(ticker_to_asset_ids, asset_id_to_symbols, market);
            }
        }
        Err(err) => {
            if strict {
                return Err(err);
            }
            tracing::warn!(
                path = %markets_config.display(),
                error = %err,
                "vault coinset scan: skipping unreadable markets config"
            );
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_label_strips_non_alnum() {
        assert_eq!(normalize_label(" wUSDC.b "), "wusdcb");
    }

    #[test]
    fn build_cat_ticker_index_lenient_continues_when_catalog_unreadable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cats = dir.path().join("cats.yaml");
        std::fs::write(&cats, "{not yaml").expect("write bad cats");
        let markets = dir.path().join("markets.yaml");
        std::fs::write(&markets, "{also bad").expect("write bad markets");
        let (tickers, symbols) = build_cat_ticker_index_lenient(&cats, &markets, None);
        assert!(tickers.is_empty());
        assert!(symbols.is_empty());
    }

    #[test]
    fn build_cat_ticker_index_lenient_keeps_cats_when_markets_unreadable() {
        let asset_id = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let dir = tempfile::tempdir().expect("tempdir");
        let cats = dir.path().join("cats.yaml");
        std::fs::write(
            &cats,
            format!(
                r"
cats:
  - asset_id: {asset_id}
    base_symbol: wUSDC.b
"
            ),
        )
        .expect("write cats");
        let markets = dir.path().join("markets.yaml");
        std::fs::write(&markets, "{not yaml").expect("write bad markets");
        let (tickers, symbols) = build_cat_ticker_index_lenient(&cats, &markets, None);
        assert!(tickers.contains_key("wusdcb"));
        assert!(symbols.contains_key(asset_id));
        assert_eq!(symbols[asset_id], vec!["wUSDC.b".to_string()]);
    }
}
