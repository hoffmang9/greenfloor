//! CAT catalog YAML load/write and Dexie metadata helpers.

use std::path::Path;

use serde_json::{json, Value as JsonValue};
use serde_yaml::Value as YamlValue;

use crate::error::{SignerError, SignerResult};
use crate::hex::{is_hex_id, normalize_hex_id};

/// Load cats catalog.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn load_cats_catalog(path: &Path) -> SignerResult<Vec<JsonValue>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(path)
        .map_err(|err| SignerError::Other(format!("failed to read {}: {err}", path.display())))?;
    let parsed: YamlValue = serde_yaml::from_str(&raw)
        .map_err(|err| SignerError::Other(format!("failed to parse {}: {err}", path.display())))?;
    let rows = parsed
        .get("cats")
        .and_then(YamlValue::as_sequence)
        .cloned()
        .unwrap_or_default();
    rows.into_iter()
        .map(|row| {
            serde_json::to_value(row)
                .map_err(|err| SignerError::Other(format!("cats row encode failed: {err}")))
        })
        .collect()
}

pub fn write_cats_catalog(path: &Path, catalog: &[JsonValue]) -> SignerResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            SignerError::Other(format!("failed to create {}: {err}", parent.display()))
        })?;
    }
    let text = serde_yaml::to_string(&json!({"cats": catalog}))
        .map_err(|err| SignerError::Other(format!("yaml encode failed: {err}")))?;
    std::fs::write(path, text)
        .map_err(|err| SignerError::Other(format!("failed to write {}: {err}", path.display())))
}

pub fn resolve_asset_id_from_catalog(catalog: &[JsonValue], ticker: &str) -> Option<String> {
    let target = ticker.trim();
    if target.is_empty() {
        return None;
    }
    catalog.iter().find_map(|row| {
        let asset_id = row
            .get("asset_id")
            .and_then(JsonValue::as_str)
            .map(normalize_hex_id)
            .filter(|value| is_hex_id(value))?;
        let base_symbol = row
            .get("base_symbol")
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        let ticker_id = row
            .get("dexie")
            .and_then(|value| value.get("ticker_id"))
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        if base_symbol.eq_ignore_ascii_case(target) || ticker_id.eq_ignore_ascii_case(target) {
            Some(asset_id)
        } else {
            None
        }
    })
}

pub fn derive_cat_metadata_from_dexie_row(row: Option<&JsonValue>) -> JsonValue {
    let Some(row) = row else {
        return json!({});
    };
    let asset_id = row
        .get("assetId")
        .or_else(|| row.get("asset_id"))
        .or_else(|| row.get("id"))
        .and_then(JsonValue::as_str)
        .map(normalize_hex_id)
        .unwrap_or_default();
    let base_symbol = row
        .get("code")
        .or_else(|| row.get("symbol"))
        .and_then(JsonValue::as_str)
        .unwrap_or("")
        .to_string();
    let name = row
        .get("name")
        .and_then(JsonValue::as_str)
        .unwrap_or(&base_symbol)
        .to_string();
    json!({
        "asset_id": asset_id,
        "base_symbol": base_symbol,
        "name": name,
        "ticker_id": row.get("ticker_id").cloned().unwrap_or(JsonValue::Null),
        "pool_id": row.get("pool_id").cloned().unwrap_or(JsonValue::Null),
        "last_price_xch": row.get("last_price_xch").cloned().unwrap_or(JsonValue::Null),
    })
}

pub fn parse_optional_float(raw: Option<&str>) -> Option<JsonValue> {
    let text = raw?.trim();
    if text.is_empty() {
        return None;
    }
    text.parse::<f64>().ok().map(|value| json!(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_asset_id_from_catalog_matches_base_symbol() {
        let catalog = vec![json!({
            "name": "My Cat",
            "base_symbol": "MCAT",
            "asset_id": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "dexie": {"ticker_id": "mcat_xch"},
        })];
        let asset_id = resolve_asset_id_from_catalog(&catalog, "MCAT").expect("match");
        assert_eq!(
            asset_id,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
        let by_ticker = resolve_asset_id_from_catalog(&catalog, "mcat_xch").expect("ticker");
        assert_eq!(by_ticker, asset_id);
    }
}
