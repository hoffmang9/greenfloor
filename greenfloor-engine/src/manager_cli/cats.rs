//! CAT catalog commands for the manager CLI.

use std::path::Path;

use serde_json::{json, Value as JsonValue};

use crate::adapters::DexieClient;
use crate::config::{is_testnet_network, resolve_dexie_base_url};
use crate::error::SignerResult;
use crate::hex::{is_hex_id, normalize_hex_id};

use super::cats_catalog::{
    derive_cat_metadata_from_dexie_row, load_cats_catalog, parse_optional_float,
    resolve_asset_id_from_catalog, write_cats_catalog,
};
use super::json::emit_json;

pub async fn run_cats_list(cats_path: &Path) -> SignerResult<i32> {
    let catalog = load_cats_catalog(cats_path)?;
    emit_json(&json!({"cats": catalog}))?;
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
        emit_json(&json!({"added": false, "error": "must_provide_cat_id_or_ticker"}))?;
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
        emit_json(&json!({"added": false, "error": "cat_id_required_and_must_be_64_hex"}))?;
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
        emit_json(&json!({"added": false, "error": "base_symbol_required"}))?;
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
        emit_json(&json!({
            "added": false,
            "error": "cat_already_exists",
            "asset_id": resolved_asset_id,
        }))?;
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
    emit_json(&json!({
        "added": true,
        "asset_id": resolved_asset_id,
        "base_symbol": resolved_symbol,
        "cats_config": cats_path.display().to_string(),
    }))?;
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
        emit_json(&json!({"deleted": false, "error": "must_provide_cat_id_or_ticker"}))?;
        return Ok(2);
    }
    let catalog = load_cats_catalog(cats_path)?;
    let mut resolved_asset_id = ref_cat_id.clone();
    if resolved_asset_id.is_empty() && !ref_ticker.is_empty() {
        resolved_asset_id = resolve_asset_id_from_catalog(&catalog, ref_ticker).unwrap_or_default();
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
        emit_json(&json!({"deleted": false, "error": "cat_id_unresolved"}))?;
        return Ok(2);
    }
    let exists = catalog.iter().any(|row| {
        row.get("asset_id")
            .and_then(JsonValue::as_str)
            .map(|value| normalize_hex_id(value) == resolved_asset_id)
            .unwrap_or(false)
    });
    if preflight_only {
        emit_json(&json!({
            "preflight": true,
            "exists": exists,
            "asset_id": resolved_asset_id,
        }))?;
        return Ok(0);
    }
    if !exists {
        emit_json(&json!({"deleted": false, "error": "cat_not_found", "asset_id": resolved_asset_id}))?;
        return Ok(2);
    }
    if !confirm_delete {
        emit_json(&json!({"deleted": false, "error": "confirmation_required", "asset_id": resolved_asset_id}))?;
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
    emit_json(&json!({"deleted": true, "asset_id": resolved_asset_id}))?;
    Ok(0)
}
