//! CAT catalog commands for the manager CLI.

use std::path::Path;

use serde_json::{json, Value as JsonValue};
use serde_yaml::Value as YamlValue;

use crate::adapters::DexieClient;
use crate::config::{is_testnet_network, resolve_dexie_base_url};
use crate::error::{SignerError, SignerResult};
use crate::hex::{is_hex_id, normalize_hex_id};

use super::json::print_json_value;

pub async fn run_cats_list(cats_path: &Path) -> SignerResult<i32> {
    let catalog = load_cats_catalog(cats_path)?;
    print_json_value(&json!({"cats": catalog})).map_err(SignerError::Other)?;
    Ok(0)
}

pub async fn run_cats_add(
    cats_path: &Path,
    network: &str,
    cat_id: Option<&str>,
    ticker: Option<&str>,
    name: Option<&str>,
    base_symbol: Option<&str>,
    ticker_id: Option<&str>,
    pool_id: Option<&str>,
    last_price_xch: Option<&str>,
    target_usd_per_unit: Option<&str>,
    use_dexie_lookup: bool,
    replace: bool,
    dexie_base_url: Option<&str>,
) -> SignerResult<i32> {
    let ref_cat_id = cat_id.map(normalize_hex_id).unwrap_or_default();
    let ref_ticker = ticker.unwrap_or("").trim();
    if ref_cat_id.is_empty() && ref_ticker.is_empty() {
        print_json_value(&json!({"added": false, "error": "must_provide_cat_id_or_ticker"}))
            .map_err(SignerError::Other)?;
        return Ok(2);
    }
    let dexie_base = resolve_dexie_base_url(network, dexie_base_url, "https://api.dexie.space")?;
    let dexie = DexieClient::new(dexie_base);
    let mut dexie_row = None;
    if use_dexie_lookup {
        if !ref_cat_id.is_empty() {
            dexie_row = dexie.lookup_token_by_cat_id(&ref_cat_id).await?;
        }
        if dexie_row.is_none() && !ref_ticker.is_empty() {
            dexie_row = dexie.lookup_token_by_symbol(ref_ticker).await?;
        }
    }
    let dexie_meta = derive_cat_metadata_from_dexie_row(dexie_row.as_ref());
    let resolved_asset_id = if !ref_cat_id.is_empty() {
        ref_cat_id.clone()
    } else {
        dexie_meta
            .get("asset_id")
            .and_then(JsonValue::as_str)
            .map(normalize_hex_id)
            .unwrap_or_default()
    };
    if !is_hex_id(&resolved_asset_id) {
        print_json_value(&json!({"added": false, "error": "cat_id_required_and_must_be_64_hex"}))
            .map_err(SignerError::Other)?;
        return Ok(2);
    }
    let resolved_symbol = base_symbol
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            dexie_meta
                .get("base_symbol")
                .and_then(JsonValue::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| ref_ticker.to_string());
    if resolved_symbol.trim().is_empty() {
        print_json_value(&json!({"added": false, "error": "base_symbol_required"}))
            .map_err(SignerError::Other)?;
        return Ok(2);
    }
    let resolved_name = name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            dexie_meta
                .get("name")
                .and_then(JsonValue::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| resolved_symbol.clone());
    let mut catalog = load_cats_catalog(cats_path)?;
    if !replace && catalog.iter().any(|row| {
        row.get("asset_id")
            .and_then(JsonValue::as_str)
            .map(|value| normalize_hex_id(value) == resolved_asset_id)
            .unwrap_or(false)
    }) {
        print_json_value(&json!({
            "added": false,
            "error": "cat_already_exists",
            "asset_id": resolved_asset_id,
        }))
        .map_err(SignerError::Other)?;
        return Ok(2);
    }
    catalog.retain(|row| {
        row.get("asset_id")
            .and_then(JsonValue::as_str)
            .map(|value| normalize_hex_id(value) != resolved_asset_id)
            .unwrap_or(true)
    });
    let mut entry = json!({
        "name": resolved_name,
        "base_symbol": resolved_symbol,
        "asset_id": resolved_asset_id,
        "target_usd_per_unit": parse_optional_float(target_usd_per_unit),
        "dexie": {
            "ticker_id": ticker_id.filter(|value| !value.is_empty()).or_else(|| dexie_meta.get("ticker_id").and_then(JsonValue::as_str)),
            "pool_id": pool_id.filter(|value| !value.is_empty()).or_else(|| dexie_meta.get("pool_id").and_then(JsonValue::as_str)),
            "last_price_xch": parse_optional_float(last_price_xch).or_else(|| dexie_meta.get("last_price_xch").cloned()),
        }
    });
    if is_testnet_network(network) {
        if let Some(obj) = entry.as_object_mut() {
            obj.insert("network".to_string(), JsonValue::String(network.to_string()));
        }
    }
    catalog.push(entry);
    write_cats_catalog(cats_path, &catalog)?;
    print_json_value(&json!({
        "added": true,
        "asset_id": resolved_asset_id,
        "base_symbol": resolved_symbol,
        "cats_config": cats_path.display().to_string(),
    }))
    .map_err(SignerError::Other)?;
    Ok(0)
}

pub async fn run_cats_delete(
    cats_path: &Path,
    network: &str,
    cat_id: Option<&str>,
    ticker: Option<&str>,
    use_dexie_lookup: bool,
    confirm_delete: bool,
    preflight_only: bool,
    dexie_base_url: Option<&str>,
) -> SignerResult<i32> {
    let ref_cat_id = cat_id.map(normalize_hex_id).unwrap_or_default();
    let ref_ticker = ticker.unwrap_or("").trim();
    if ref_cat_id.is_empty() && ref_ticker.is_empty() {
        print_json_value(&json!({"deleted": false, "error": "must_provide_cat_id_or_ticker"}))
            .map_err(SignerError::Other)?;
        return Ok(2);
    }
    let catalog = load_cats_catalog(cats_path)?;
    let mut resolved_asset_id = ref_cat_id.clone();
    if resolved_asset_id.is_empty() && !ref_ticker.is_empty() {
        resolved_asset_id = resolve_asset_id_from_catalog(&catalog, ref_ticker)
            .unwrap_or_default();
    }
    if resolved_asset_id.is_empty() && use_dexie_lookup && !ref_ticker.is_empty() {
        let dexie_base = resolve_dexie_base_url(network, dexie_base_url, "https://api.dexie.space")?;
        let dexie = DexieClient::new(dexie_base);
        if let Some(row) = dexie.lookup_token_by_symbol(ref_ticker).await? {
            let meta = derive_cat_metadata_from_dexie_row(Some(&row));
            resolved_asset_id = meta
                .get("asset_id")
                .and_then(JsonValue::as_str)
                .map(normalize_hex_id)
                .unwrap_or_default();
        }
    }
    if resolved_asset_id.is_empty() {
        print_json_value(&json!({"deleted": false, "error": "cat_id_unresolved"}))
            .map_err(SignerError::Other)?;
        return Ok(2);
    }
    let exists = catalog.iter().any(|row| {
        row.get("asset_id")
            .and_then(JsonValue::as_str)
            .map(|value| normalize_hex_id(value) == resolved_asset_id)
            .unwrap_or(false)
    });
    if preflight_only {
        print_json_value(&json!({
            "preflight": true,
            "exists": exists,
            "asset_id": resolved_asset_id,
        }))
        .map_err(SignerError::Other)?;
        return Ok(0);
    }
    if !exists {
        print_json_value(&json!({"deleted": false, "error": "cat_not_found", "asset_id": resolved_asset_id}))
            .map_err(SignerError::Other)?;
        return Ok(2);
    }
    if !confirm_delete {
        print_json_value(&json!({"deleted": false, "error": "confirmation_required", "asset_id": resolved_asset_id}))
            .map_err(SignerError::Other)?;
        return Ok(2);
    }
    let updated: Vec<JsonValue> = catalog
        .into_iter()
        .filter(|row| {
            row.get("asset_id")
                .and_then(JsonValue::as_str)
                .map(|value| normalize_hex_id(value) != resolved_asset_id)
                .unwrap_or(true)
        })
        .collect();
    write_cats_catalog(cats_path, &updated)?;
    print_json_value(&json!({"deleted": true, "asset_id": resolved_asset_id}))
        .map_err(SignerError::Other)?;
    Ok(0)
}

fn load_cats_catalog(path: &Path) -> SignerResult<Vec<JsonValue>> {
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

fn write_cats_catalog(path: &Path, catalog: &[JsonValue]) -> SignerResult<()> {
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

fn resolve_asset_id_from_catalog(catalog: &[JsonValue], ticker: &str) -> Option<String> {
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

fn derive_cat_metadata_from_dexie_row(row: Option<&JsonValue>) -> JsonValue {
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

fn parse_optional_float(raw: Option<&str>) -> Option<JsonValue> {
    let text = raw?.trim();
    if text.is_empty() {
        return None;
    }
    text.parse::<f64>()
        .ok()
        .map(|value| json!(value))
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
