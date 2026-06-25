//! CAT catalog Dexie metadata helpers (YAML IO lives in [`crate::config::cats_catalog`]).

use serde_json::{json, Value as JsonValue};

use crate::config::{build_cat_ticker_index_from_cats_rows, lookup_asset_id_from_ticker};
use crate::hex::normalize_hex_id;

pub use crate::config::{load_cats_catalog, write_cats_catalog};

pub fn resolve_asset_id_from_catalog(catalog: &[JsonValue], ticker: &str) -> Option<String> {
    let index = build_cat_ticker_index_from_cats_rows(catalog);
    lookup_asset_id_from_ticker(&index, ticker).ok().flatten()
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
