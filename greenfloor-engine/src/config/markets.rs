use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use serde_json::{json, Value};

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
    quote_asset_type: Option<String>,
    receive_address: Option<String>,
    signer_key_id: Option<String>,
    mode: Option<String>,
    pricing: Option<Value>,
    ladders: Option<HashMap<String, Vec<LadderEntryYaml>>>,
}

#[derive(Debug, Deserialize)]
struct LadderEntryYaml {
    size_base_units: Option<i64>,
    target_count: Option<i64>,
    split_buffer_count: Option<i64>,
    combine_when_excess_factor: Option<f64>,
}

pub fn load_markets_config(path: &Path) -> SignerResult<MarketsConfig> {
    load_markets_config_with_overlay(path, None)
}

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

pub fn parse_markets_config(raw: &Value) -> SignerResult<MarketsConfig> {
    let parsed: MarketsYaml = serde_json::from_value(raw.clone())
        .map_err(|err| SignerError::Other(format!("invalid markets config shape: {err}")))?;
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
                        combine_when_excess_factor: entry.combine_when_excess_factor.unwrap_or(2.0),
                    })
                    .collect();
                ladders.insert(side, parsed_entries);
            }
        }
        let base_asset = row.base_asset.unwrap_or_default().trim().to_string();
        let quote_asset = row.quote_asset.unwrap_or_default().trim().to_string();
        let quote_asset_type = row
            .quote_asset_type
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let mut pricing = row.pricing.clone().unwrap_or_else(|| json!({}));
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
        let cancel_move_threshold_bps = pop_cancel_move_threshold_bps(&mut pricing);
        markets.push(MarketConfig {
            market_id,
            enabled: row.enabled.unwrap_or(false),
            base_asset,
            base_symbol: row.base_symbol.unwrap_or_default().trim().to_string(),
            quote_asset,
            quote_asset_type,
            receive_address: row.receive_address.unwrap_or_default().trim().to_string(),
            signer_key_id: row.signer_key_id.unwrap_or_default().trim().to_string(),
            mode: row
                .mode
                .unwrap_or_else(|| "sell_only".to_string())
                .trim()
                .to_ascii_lowercase(),
            pricing,
            cancel_move_threshold_bps,
            ladders,
        });
    }
    Ok(MarketsConfig { markets })
}

pub fn cancel_policy_stable_vs_unstable(pricing: &Value) -> bool {
    pricing
        .get("cancel_policy_stable_vs_unstable")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base_market_row() -> Value {
        json!({
            "id": "m1",
            "enabled": true,
            "base_asset": "asset1",
            "base_symbol": "AS1",
            "quote_asset": "xch",
            "quote_asset_type": "unstable",
            "receive_address": "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
            "mode": "sell_only",
            "signer_key_id": "key-main-1",
            "inventory": {"low_watermark_base_units": 1},
            "ladders": {"sell": [{"size_base_units": 1, "target_count": 1}]},
            "pricing": {},
        })
    }

    fn parse_single_market(row: Value) -> SignerResult<MarketConfig> {
        parse_markets_config(&json!({"markets": [row]})).map(|cfg| {
            cfg.markets
                .into_iter()
                .next()
                .expect("single market expected")
        })
    }

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
    fn parse_markets_config_rejects_invalid_strategy_spread() {
        let mut row = base_market_row();
        row["pricing"] = json!({"strategy_target_spread_bps": 0});
        let err = parse_single_market(row).expect_err("invalid spread");
        assert!(err.to_string().contains("strategy_target_spread_bps"));
    }

    #[test]
    fn parse_markets_config_rejects_invalid_strategy_price_band() {
        let mut row = base_market_row();
        row["pricing"] = json!({
            "strategy_min_xch_price_usd": 50.0,
            "strategy_max_xch_price_usd": 40.0,
        });
        let err = parse_single_market(row).expect_err("invalid price band");
        assert!(err
            .to_string()
            .contains("strategy_min_xch_price_usd must be <= strategy_max_xch_price_usd"));
    }

    #[test]
    fn parse_markets_config_accepts_valid_strategy_controls() {
        let mut row = base_market_row();
        row["pricing"] = json!({
            "strategy_target_spread_bps": 120,
            "strategy_min_xch_price_usd": 20.0,
            "strategy_max_xch_price_usd": 60.0,
        });
        let market = parse_single_market(row).expect("valid strategy controls");
        assert_eq!(
            market.pricing["strategy_target_spread_bps"].as_i64(),
            Some(120)
        );
    }

    #[test]
    fn parse_markets_config_rejects_legacy_strategy_expiry_fields() {
        let mut row = base_market_row();
        row["pricing"] = json!({
            "strategy_offer_expiry_unit": "hours",
            "strategy_offer_expiry_value": 2,
        });
        let err = parse_single_market(row).expect_err("legacy expiry fields");
        assert!(err
            .to_string()
            .contains("strategy_offer_expiry_unit/value are no longer supported"));
    }

    #[test]
    fn parse_markets_config_rejects_invalid_strategy_expiry_minutes_type() {
        let mut row = base_market_row();
        row["pricing"] = json!({"strategy_offer_expiry_minutes": "abc"});
        let err = parse_single_market(row).expect_err("invalid expiry minutes type");
        assert!(err
            .to_string()
            .contains("strategy_offer_expiry_minutes must be an integer"));
    }

    #[test]
    fn parse_markets_config_accepts_strategy_expiry_override() {
        let mut row = base_market_row();
        row["quote_asset_type"] = json!("stable");
        row["pricing"] = json!({"strategy_offer_expiry_minutes": 120});
        let market = parse_single_market(row).expect("strategy expiry override");
        assert_eq!(
            market.pricing["strategy_offer_expiry_minutes"].as_i64(),
            Some(120)
        );
    }

    #[test]
    fn parse_markets_config_accepts_unstable_expiry_above_15_minutes() {
        let mut row = base_market_row();
        row["pricing"] = json!({"strategy_offer_expiry_minutes": 30});
        let market = parse_single_market(row).expect("unstable expiry above 15 minutes");
        assert_eq!(
            market.pricing["strategy_offer_expiry_minutes"].as_i64(),
            Some(30)
        );
    }

    #[test]
    fn parse_markets_config_rejects_legacy_reference_fields() {
        let mut row = base_market_row();
        row["pricing"] = json!({
            "reference_source": "coingecko",
            "reference_pair": "xch_usd",
        });
        let err = parse_single_market(row).expect_err("legacy reference fields");
        assert!(err
            .to_string()
            .contains("reference_source is no longer supported"));
    }

    #[test]
    fn parse_markets_config_rejects_invalid_cancel_move_threshold_bps() {
        let mut row = base_market_row();
        row["pricing"] = json!({"cancel_move_threshold_bps": 0});
        let err = parse_single_market(row).expect_err("invalid cancel threshold");
        assert!(err
            .to_string()
            .contains("cancel_move_threshold_bps must be positive"));
    }

    #[test]
    fn parse_markets_config_accepts_cancel_move_threshold_bps() {
        let mut row = base_market_row();
        row["pricing"] = json!({"cancel_move_threshold_bps": 250});
        let market = parse_single_market(row).expect("cancel threshold");
        assert!(market.pricing.get("cancel_move_threshold_bps").is_none());
        assert_eq!(market.cancel_move_threshold_bps, Some(250));
    }

    #[test]
    fn parse_markets_config_stable_quote_validates_present_strategy_fields() {
        let mut row = base_market_row();
        row["quote_asset_type"] = json!("stable");
        row["pricing"] = json!({
            "strategy_target_spread_bps": 0,
            "strategy_min_xch_price_usd": -1,
            "strategy_max_xch_price_usd": "invalid",
        });
        let err = parse_single_market(row).expect_err("stable strategy validation");
        assert!(err.to_string().contains("strategy_target_spread_bps"));
    }

    #[test]
    fn parse_markets_config_defaults_cat_unit_multipliers_to_1000() {
        let mut row = base_market_row();
        row["base_asset"] = json!("BYC");
        row["quote_asset"] = json!("wUSDC.b");
        let market = parse_single_market(row).expect("cat multipliers");
        assert_eq!(
            market.pricing["base_unit_mojo_multiplier"].as_i64(),
            Some(1000)
        );
        assert_eq!(
            market.pricing["quote_unit_mojo_multiplier"].as_i64(),
            Some(1000)
        );
    }

    #[test]
    fn parse_markets_config_defaults_xch_quote_multiplier_to_one_trillion() {
        let row = base_market_row();
        let market = parse_single_market(row).expect("xch quote multiplier");
        assert_eq!(
            market.pricing["base_unit_mojo_multiplier"].as_i64(),
            Some(1000)
        );
        assert_eq!(
            market.pricing["quote_unit_mojo_multiplier"].as_i64(),
            Some(1_000_000_000_000)
        );
    }

    #[test]
    fn parse_markets_config_rejects_noncanonical_cat_base_multiplier() {
        let mut row = base_market_row();
        row["base_asset"] = json!("BYC");
        row["pricing"] = json!({"base_unit_mojo_multiplier": 10});
        let err = parse_single_market(row).expect_err("noncanonical base multiplier");
        assert!(err
            .to_string()
            .contains("base_unit_mojo_multiplier must be 1000 for CAT assets"));
    }

    #[test]
    fn parse_markets_config_rejects_noncanonical_cat_quote_multiplier() {
        let mut row = base_market_row();
        row["quote_asset"] = json!("wUSDC.b");
        row["pricing"] = json!({"quote_unit_mojo_multiplier": 10});
        let err = parse_single_market(row).expect_err("noncanonical quote multiplier");
        assert!(err
            .to_string()
            .contains("quote_unit_mojo_multiplier must be 1000 for CAT assets"));
    }

    #[test]
    fn parse_markets_config_preserves_xch_multiplier_override() {
        let mut row = base_market_row();
        row["pricing"] = json!({"quote_unit_mojo_multiplier": 1_000_000_000_000_i64});
        let market = parse_single_market(row).expect("xch multiplier override");
        assert_eq!(
            market.pricing["quote_unit_mojo_multiplier"].as_i64(),
            Some(1_000_000_000_000)
        );
    }

    #[test]
    fn rejects_testnet_receive_address_in_base_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("markets.yaml");
        std::fs::write(
            &path,
            r#"markets:
  - id: bad_base
    enabled: true
    base_asset: "a1"
    base_symbol: "A1"
    quote_asset: "xch"
    quote_asset_type: "unstable"
    signer_key_id: "key-main-1"
    receive_address: "txch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
    inventory:
      low_watermark_base_units: 100
"#,
        )
        .expect("write markets");
        let err = load_markets_config_with_overlay(&path, None).expect_err("txch in base");
        assert!(err.to_string().contains("testnet receive_address"));
    }

    #[test]
    fn resolves_market_by_id() {
        let markets = sample_markets();
        let market =
            resolve_market_for_build(&markets, Some("m1"), None, "mainnet").expect("market");
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
