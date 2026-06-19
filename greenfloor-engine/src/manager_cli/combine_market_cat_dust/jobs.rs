use std::collections::{BTreeMap, HashSet};

use serde_json::Value;

use crate::config::MarketsConfig;
use crate::error::{SignerError, SignerResult};
use crate::hex::{is_hex_id, normalize_hex_id};
use crate::manager_cli::cats_catalog::{load_cats_catalog, resolve_asset_id_from_catalog};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatDustJob {
    pub cat_asset_id: String,
    pub signer_key_id: String,
    pub receive_address: String,
    pub market_ids: Vec<String>,
}

pub fn resolve_market_base_cat_asset_id(
    base_asset: &str,
    base_symbol: &str,
    catalog: &[Value],
) -> Option<String> {
    let normalized = normalize_hex_id(base_asset);
    if is_hex_id(&normalized) {
        return Some(normalized);
    }
    resolve_asset_id_from_catalog(catalog, base_asset)
        .or_else(|| resolve_asset_id_from_catalog(catalog, base_symbol))
}

pub fn build_enabled_cat_jobs(
    markets: &MarketsConfig,
    cats_config_path: &std::path::Path,
    only_cat_asset_id: Option<&str>,
) -> SignerResult<Vec<CatDustJob>> {
    let catalog = load_cats_catalog(cats_config_path)?;
    let filter_id = only_cat_asset_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize_hex_id)
        .filter(|value| is_hex_id(value));

    let mut grouped: BTreeMap<(String, String), (String, Vec<String>)> = BTreeMap::new();
    for market in markets.markets.iter().filter(|market| market.enabled) {
        let Some(asset_id) =
            resolve_market_base_cat_asset_id(&market.base_asset, &market.base_symbol, &catalog)
        else {
            continue;
        };
        if filter_id.as_ref().is_some_and(|filter| filter != &asset_id) {
            continue;
        }
        let signer_key_id = market.signer_key_id.trim().to_string();
        let market_id = market.market_id.trim().to_string();
        let receive_address = market.receive_address.trim().to_string();
        let key = (signer_key_id.clone(), asset_id.clone());
        match grouped.get_mut(&key) {
            None => {
                grouped.insert(key, (receive_address, vec![market_id]));
            }
            Some((existing_receive, market_ids)) => {
                if existing_receive != &receive_address {
                    return Err(SignerError::Other(format!(
                        "Conflicting receive_address for signer={signer_key_id:?} cat={asset_id}: \
                         {existing_receive:?} vs {receive_address:?} (markets {market_ids:?} vs [{market_id:?}])"
                    )));
                }
                market_ids.push(market_id);
            }
        }
    }

    let mut jobs = Vec::new();
    for ((signer_key_id, cat_asset_id), (receive_address, market_ids)) in grouped {
        let mut unique: Vec<String> = market_ids
            .into_iter()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        unique.sort();
        jobs.push(CatDustJob {
            cat_asset_id,
            signer_key_id,
            receive_address,
            market_ids: unique,
        });
    }
    jobs.sort_by(|left, right| {
        left.signer_key_id
            .cmp(&right.signer_key_id)
            .then_with(|| left.cat_asset_id.cmp(&right.cat_asset_id))
    });
    Ok(jobs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::load_markets_config;
    use serde_json::json;
    use std::collections::HashMap;

    fn write_markets(path: &std::path::Path, body: &str) {
        std::fs::write(path, body).expect("write markets");
    }

    fn write_cats(path: &std::path::Path, body: &str) {
        std::fs::write(path, body).expect("write cats");
    }

    #[test]
    fn build_enabled_cat_jobs_resolves_symbol_and_merges() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cat_hex = "f".repeat(64);
        let zzt = "e".repeat(64);
        let markets = dir.path().join("markets.yaml");
        write_markets(
            &markets,
            &format!(
                r#"markets:
  - id: hex_m
    enabled: true
    base_asset: "{cat_hex}"
    base_symbol: HEX
    quote_asset: xch
    quote_asset_type: unstable
    signer_key_id: key-a
    receive_address: xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h
    mode: sell_only
    inventory:
      low_watermark_base_units: 100
    pricing:
      min_price_quote_per_base: 0.0031
      max_price_quote_per_base: 0.0038
  - id: sym_m
    enabled: true
    base_asset: ZZT
    base_symbol: ZZT
    quote_asset: xch
    quote_asset_type: unstable
    signer_key_id: key-a
    receive_address: xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h
    mode: sell_only
    inventory:
      low_watermark_base_units: 100
    pricing:
      min_price_quote_per_base: 0.0031
      max_price_quote_per_base: 0.0038
  - id: off_m
    enabled: false
    base_asset: "{cat_hex}"
    base_symbol: HEX2
    quote_asset: xch
    quote_asset_type: unstable
    signer_key_id: key-a
    receive_address: xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h
    mode: sell_only
    inventory:
      low_watermark_base_units: 100
    pricing:
      min_price_quote_per_base: 0.0031
      max_price_quote_per_base: 0.0038
"#
            ),
        );
        let cats = dir.path().join("cats.yaml");
        write_cats(
            &cats,
            &format!("cats:\n  - name: z\n    base_symbol: ZZT\n    asset_id: \"{zzt}\"\n"),
        );
        let catalog = load_cats_catalog(&cats).expect("load cats");
        assert_eq!(catalog.len(), 1, "catalog: {catalog:?}");
        assert_eq!(
            resolve_asset_id_from_catalog(&catalog, "ZZT"),
            Some(zzt.clone())
        );
        let jobs = build_enabled_cat_jobs(
            &load_markets_config(&markets).expect("markets"),
            &cats,
            None,
        )
        .expect("build enabled cat jobs");
        let by_cat: HashMap<_, _> = jobs
            .into_iter()
            .map(|job| (job.cat_asset_id.clone(), job))
            .collect();
        assert_eq!(by_cat.len(), 2);
        assert!(by_cat.contains_key(&cat_hex));
        assert!(by_cat.contains_key(&zzt));
        assert_eq!(by_cat[&cat_hex].signer_key_id, "key-a");
        assert_eq!(by_cat[&zzt].market_ids, vec!["sym_m".to_string()]);
    }

    #[test]
    fn build_enabled_cat_jobs_conflict_receive_address() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cat_hex = "f".repeat(64);
        let markets = dir.path().join("markets.yaml");
        write_markets(
            &markets,
            &format!(
                r#"markets:
  - id: m1
    enabled: true
    base_asset: "{cat_hex}"
    base_symbol: HEX
    quote_asset: xch
    quote_asset_type: unstable
    signer_key_id: key-a
    receive_address: xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h
    mode: sell_only
    inventory:
      low_watermark_base_units: 100
    pricing:
      min_price_quote_per_base: 0.0031
      max_price_quote_per_base: 0.0038
  - id: m2
    enabled: true
    base_asset: "{cat_hex}"
    base_symbol: HEX
    quote_asset: xch
    quote_asset_type: unstable
    signer_key_id: key-a
    receive_address: xch1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq2u30w
    mode: sell_only
    inventory:
      low_watermark_base_units: 100
    pricing:
      min_price_quote_per_base: 0.0031
      max_price_quote_per_base: 0.0038
"#
            ),
        );
        let cats = dir.path().join("cats.yaml");
        write_cats(&cats, "cats: []\n");
        let err = build_enabled_cat_jobs(
            &load_markets_config(&markets).expect("markets"),
            &cats,
            None,
        )
        .expect_err("conflict");
        assert!(err.to_string().contains("Conflicting receive_address"));
    }

    #[test]
    fn resolve_market_base_cat_asset_id_uses_catalog_symbol() {
        let zzt = "e".repeat(64);
        let catalog = vec![json!({"base_symbol": "ZZT", "asset_id": zzt})];
        assert_eq!(
            resolve_market_base_cat_asset_id("ZZT", "ZZT", &catalog),
            Some(zzt.clone())
        );
    }
}
