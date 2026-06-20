//! CAT catalog commands for the manager CLI.

use serde_json::{json, Value as JsonValue};

use crate::adapters::DexieClient;
use crate::config::{is_testnet_network, resolve_dexie_base_url};
use crate::error::SignerResult;
use crate::hex::{is_hex_id, normalize_hex_id};

use super::cats_catalog::{
    derive_cat_metadata_from_dexie_row, load_cats_catalog, parse_optional_float,
    resolve_asset_id_from_catalog, write_cats_catalog,
};
use super::context::ManagerContext;

pub struct CatsAddRequest<'a> {
    pub ctx: &'a ManagerContext,
    pub network: &'a str,
    pub cat_id: Option<&'a str>,
    pub ticker: Option<&'a str>,
    pub name: Option<&'a str>,
    pub base_symbol: Option<&'a str>,
    pub ticker_id: Option<&'a str>,
    pub pool_id: Option<&'a str>,
    pub last_price_xch: Option<&'a str>,
    pub target_usd_per_unit: Option<&'a str>,
    pub use_dexie_lookup: bool,
    pub replace: bool,
}

struct ResolvedCatsAddFields {
    asset_id: String,
    base_symbol: String,
    name: String,
}

pub fn run_cats_list(ctx: &ManagerContext) -> SignerResult<i32> {
    let catalog = load_cats_catalog(&ctx.cats_config)?;
    ctx.emit_json(&json!({"cats": catalog}))?;
    Ok(0)
}

async fn lookup_dexie_cat_row(
    ctx: &ManagerContext,
    network: &str,
    ref_cat_id: &str,
    ref_ticker: &str,
    use_dexie_lookup: bool,
) -> SignerResult<Option<serde_json::Value>> {
    if !use_dexie_lookup {
        return Ok(None);
    }
    let dexie_base = resolve_dexie_base_url(
        network,
        ctx.dexie_base_url.as_deref(),
        "https://api.dexie.space",
    )?;
    let dexie = DexieClient::new(dexie_base);
    let mut dexie_row = None;
    if !ref_cat_id.is_empty() {
        dexie_row = dexie.lookup_token_by_cat_id(ref_cat_id).await?;
    }
    if dexie_row.is_none() && !ref_ticker.is_empty() {
        dexie_row = dexie.lookup_token_by_symbol(ref_ticker).await?;
    }
    Ok(dexie_row)
}

fn resolve_cats_add_fields(
    ref_cat_id: &str,
    ref_ticker: &str,
    dexie_meta: &JsonValue,
    base_symbol: Option<&str>,
    name: Option<&str>,
) -> Result<ResolvedCatsAddFields, &'static str> {
    let resolved_asset_id = if ref_cat_id.is_empty() {
        dexie_meta
            .get("asset_id")
            .and_then(JsonValue::as_str)
            .map(normalize_hex_id)
            .unwrap_or_default()
    } else {
        ref_cat_id.to_string()
    };
    if !is_hex_id(&resolved_asset_id) {
        return Err("cat_id_required_and_must_be_64_hex");
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
        return Err("base_symbol_required");
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
    Ok(ResolvedCatsAddFields {
        asset_id: resolved_asset_id,
        base_symbol: resolved_symbol,
        name: resolved_name,
    })
}

fn build_cats_catalog_entry(
    network: &str,
    fields: &ResolvedCatsAddFields,
    dexie_meta: &JsonValue,
    ticker_id: Option<&str>,
    pool_id: Option<&str>,
    last_price_xch: Option<&str>,
    target_usd_per_unit: Option<&str>,
) -> JsonValue {
    let mut entry = json!({
        "name": fields.name,
        "base_symbol": fields.base_symbol,
        "asset_id": fields.asset_id,
        "target_usd_per_unit": parse_optional_float(target_usd_per_unit),
        "dexie": {
            "ticker_id": ticker_id.filter(|value| !value.is_empty()).or_else(|| dexie_meta.get("ticker_id").and_then(JsonValue::as_str)),
            "pool_id": pool_id.filter(|value| !value.is_empty()).or_else(|| dexie_meta.get("pool_id").and_then(JsonValue::as_str)),
            "last_price_xch": parse_optional_float(last_price_xch).or_else(|| dexie_meta.get("last_price_xch").cloned()),
        }
    });
    if is_testnet_network(network) {
        if let Some(obj) = entry.as_object_mut() {
            obj.insert(
                "network".to_string(),
                JsonValue::String(network.to_string()),
            );
        }
    }
    entry
}

pub async fn run_cats_add(request: CatsAddRequest<'_>) -> SignerResult<i32> {
    let CatsAddRequest {
        ctx,
        network,
        cat_id,
        ticker,
        name,
        base_symbol,
        ticker_id,
        pool_id,
        last_price_xch,
        target_usd_per_unit,
        use_dexie_lookup,
        replace,
    } = request;
    let ref_cat_id = cat_id.map(normalize_hex_id).unwrap_or_default();
    let ref_ticker = ticker.unwrap_or("").trim();
    if ref_cat_id.is_empty() && ref_ticker.is_empty() {
        ctx.emit_json(&json!({"added": false, "error": "must_provide_cat_id_or_ticker"}))?;
        return Ok(2);
    }
    let dexie_row =
        lookup_dexie_cat_row(ctx, network, &ref_cat_id, ref_ticker, use_dexie_lookup).await?;
    let dexie_meta = derive_cat_metadata_from_dexie_row(dexie_row.as_ref());
    let fields =
        match resolve_cats_add_fields(&ref_cat_id, ref_ticker, &dexie_meta, base_symbol, name) {
            Ok(fields) => fields,
            Err(error) => {
                ctx.emit_json(&json!({"added": false, "error": error}))?;
                return Ok(2);
            }
        };

    let mut catalog = load_cats_catalog(&ctx.cats_config)?;
    if !replace
        && catalog.iter().any(|row| {
            row.get("asset_id")
                .and_then(JsonValue::as_str)
                .is_some_and(|value| normalize_hex_id(value) == fields.asset_id)
        })
    {
        ctx.emit_json(&json!({
            "added": false,
            "error": "cat_already_exists",
            "asset_id": fields.asset_id,
        }))?;
        return Ok(2);
    }
    catalog.retain(|row| {
        row.get("asset_id")
            .and_then(JsonValue::as_str)
            .is_none_or(|value| normalize_hex_id(value) != fields.asset_id)
    });
    catalog.push(build_cats_catalog_entry(
        network,
        &fields,
        &dexie_meta,
        ticker_id,
        pool_id,
        last_price_xch,
        target_usd_per_unit,
    ));
    write_cats_catalog(&ctx.cats_config, &catalog)?;
    ctx.emit_json(&json!({
        "added": true,
        "asset_id": fields.asset_id,
        "base_symbol": fields.base_symbol,
        "cats_config": ctx.cats_config.display().to_string(),
    }))?;
    Ok(0)
}

pub async fn run_cats_delete(
    ctx: &ManagerContext,
    network: &str,
    cat_id: Option<&str>,
    ticker: Option<&str>,
    use_dexie_lookup: bool,
    confirm_delete: bool,
    preflight_only: bool,
) -> SignerResult<i32> {
    let ref_cat_id = cat_id.map(normalize_hex_id).unwrap_or_default();
    let ref_ticker = ticker.unwrap_or("").trim();
    if ref_cat_id.is_empty() && ref_ticker.is_empty() {
        ctx.emit_json(&json!({"deleted": false, "error": "must_provide_cat_id_or_ticker"}))?;
        return Ok(2);
    }
    let catalog = load_cats_catalog(&ctx.cats_config)?;
    let mut resolved_asset_id = ref_cat_id.clone();
    if resolved_asset_id.is_empty() && !ref_ticker.is_empty() {
        resolved_asset_id = resolve_asset_id_from_catalog(&catalog, ref_ticker).unwrap_or_default();
    }
    if resolved_asset_id.is_empty() && use_dexie_lookup && !ref_ticker.is_empty() {
        let dexie_base = resolve_dexie_base_url(
            network,
            ctx.dexie_base_url.as_deref(),
            "https://api.dexie.space",
        )?;
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
        ctx.emit_json(&json!({"deleted": false, "error": "cat_id_unresolved"}))?;
        return Ok(2);
    }
    let exists = catalog.iter().any(|row| {
        row.get("asset_id")
            .and_then(JsonValue::as_str)
            .is_some_and(|value| normalize_hex_id(value) == resolved_asset_id)
    });
    if preflight_only {
        ctx.emit_json(&json!({
            "preflight": true,
            "exists": exists,
            "asset_id": resolved_asset_id,
        }))?;
        return Ok(0);
    }
    if !exists {
        ctx.emit_json(
            &json!({"deleted": false, "error": "cat_not_found", "asset_id": resolved_asset_id}),
        )?;
        return Ok(2);
    }
    if !confirm_delete {
        ctx.emit_json(&json!({"deleted": false, "error": "confirmation_required", "asset_id": resolved_asset_id}))?;
        return Ok(2);
    }
    let updated: Vec<JsonValue> = catalog
        .into_iter()
        .filter(|row| {
            row.get("asset_id")
                .and_then(JsonValue::as_str)
                .is_none_or(|value| normalize_hex_id(value) != resolved_asset_id)
        })
        .collect();
    write_cats_catalog(&ctx.cats_config, &updated)?;
    ctx.emit_json(&json!({"deleted": true, "asset_id": resolved_asset_id}))?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager_cli::context::ManagerContext;
    use crate::manager_cli::json::ManagerOutput;

    fn cats_test_context(
        dir: &tempfile::TempDir,
    ) -> (
        ManagerContext,
        std::sync::Arc<std::sync::Mutex<Vec<JsonValue>>>,
    ) {
        let cats_path = dir.path().join("cats.yaml");
        let (output, captured) = ManagerOutput::capturing(true);
        let ctx = ManagerContext::for_test_with_cats(
            dir.path().join("unused-program.yaml"),
            dir.path().join("unused-markets.yaml"),
            cats_path,
            output,
        );
        (ctx, captured)
    }

    fn pop_captured(captured: &std::sync::Arc<std::sync::Mutex<Vec<JsonValue>>>) -> JsonValue {
        captured
            .lock()
            .expect("capture lock")
            .pop()
            .expect("json emitted")
    }

    fn cats_list_payload(
        ctx: &ManagerContext,
        captured: &std::sync::Arc<std::sync::Mutex<Vec<JsonValue>>>,
    ) -> JsonValue {
        let code = run_cats_list(ctx).expect("cats-list");
        assert_eq!(code, 0);
        pop_captured(captured)
    }

    #[tokio::test]
    async fn cats_add_manual_without_dexie_lookup() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (ctx, captured) = cats_test_context(&dir);
        let code = run_cats_add(CatsAddRequest {
            ctx: &ctx,
            network: "mainnet",
            cat_id: Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
            ticker: None,
            name: Some("Manual CAT"),
            base_symbol: Some("MCAT"),
            ticker_id: Some("manualcat_xch"),
            pool_id: Some("pool-manual"),
            last_price_xch: Some("0.42"),
            target_usd_per_unit: Some("4.2"),
            use_dexie_lookup: false,
            replace: false,
        })
        .await
        .expect("cats-add");
        assert_eq!(code, 0);
        let add_payload = pop_captured(&captured);
        assert_eq!(add_payload.get("added"), Some(&json!(true)));
        let payload = cats_list_payload(&ctx, &captured);
        let rows = payload
            .get("cats")
            .and_then(|v| v.as_array())
            .expect("cats");
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.get("name"), Some(&json!("Manual CAT")));
        assert_eq!(row.get("base_symbol"), Some(&json!("MCAT")));
        assert_eq!(
            row.get("asset_id"),
            Some(&json!(
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
            ))
        );
    }

    #[tokio::test]
    async fn cats_add_replace_required_for_existing_asset() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (ctx, captured) = cats_test_context(&dir);
        let cat_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let first = run_cats_add(CatsAddRequest {
            ctx: &ctx,
            network: "mainnet",
            cat_id: Some(cat_id),
            ticker: None,
            name: Some("First"),
            base_symbol: Some("ONE"),
            ticker_id: None,
            pool_id: None,
            last_price_xch: None,
            target_usd_per_unit: None,
            use_dexie_lookup: false,
            replace: false,
        })
        .await
        .expect("first add");
        assert_eq!(first, 0);
        let _ = pop_captured(&captured);
        let second = run_cats_add(CatsAddRequest {
            ctx: &ctx,
            network: "mainnet",
            cat_id: Some(cat_id),
            ticker: None,
            name: Some("Second"),
            base_symbol: Some("TWO"),
            ticker_id: None,
            pool_id: None,
            last_price_xch: None,
            target_usd_per_unit: None,
            use_dexie_lookup: false,
            replace: false,
        })
        .await
        .expect("second add");
        assert_eq!(second, 2);
        let payload = pop_captured(&captured);
        assert_eq!(payload.get("error"), Some(&json!("cat_already_exists")));
    }

    #[tokio::test]
    async fn cats_delete_by_cat_id() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (ctx, captured) = cats_test_context(&dir);
        let cat_id = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
        let added = run_cats_add(CatsAddRequest {
            ctx: &ctx,
            network: "mainnet",
            cat_id: Some(cat_id),
            ticker: None,
            name: Some("Delete Me"),
            base_symbol: Some("DEL"),
            ticker_id: None,
            pool_id: None,
            last_price_xch: None,
            target_usd_per_unit: None,
            use_dexie_lookup: false,
            replace: false,
        })
        .await
        .expect("cats-add");
        assert_eq!(added, 0);
        let _ = pop_captured(&captured);
        let deleted = run_cats_delete(&ctx, "mainnet", Some(cat_id), None, false, true, false)
            .await
            .expect("cats-delete");
        assert_eq!(deleted, 0);
        let payload = pop_captured(&captured);
        assert_eq!(payload.get("deleted"), Some(&json!(true)));
        assert!(cats_list_payload(&ctx, &captured)
            .get("cats")
            .and_then(|v| v.as_array())
            .is_some_and(std::vec::Vec::is_empty));
    }

    #[tokio::test]
    async fn cats_delete_requires_confirmation_when_not_yes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (ctx, captured) = cats_test_context(&dir);
        let cat_id = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        let added = run_cats_add(CatsAddRequest {
            ctx: &ctx,
            network: "mainnet",
            cat_id: Some(cat_id),
            ticker: None,
            name: Some("Needs Confirm"),
            base_symbol: Some("CNF"),
            ticker_id: None,
            pool_id: None,
            last_price_xch: None,
            target_usd_per_unit: None,
            use_dexie_lookup: false,
            replace: false,
        })
        .await
        .expect("cats-add");
        assert_eq!(added, 0);
        let _ = pop_captured(&captured);
        let deleted = run_cats_delete(&ctx, "mainnet", Some(cat_id), None, false, false, false)
            .await
            .expect("cats-delete");
        assert_eq!(deleted, 2);
        let payload = pop_captured(&captured);
        assert_eq!(payload.get("error"), Some(&json!("confirmation_required")));
    }

    #[tokio::test]
    async fn cats_delete_preflight_only_does_not_delete() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (ctx, captured) = cats_test_context(&dir);
        let cat_id = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        let added = run_cats_add(CatsAddRequest {
            ctx: &ctx,
            network: "mainnet",
            cat_id: Some(cat_id),
            ticker: None,
            name: Some("Preflight Only"),
            base_symbol: Some("PFL"),
            ticker_id: None,
            pool_id: None,
            last_price_xch: None,
            target_usd_per_unit: None,
            use_dexie_lookup: false,
            replace: false,
        })
        .await
        .expect("cats-add");
        assert_eq!(added, 0);
        let _ = pop_captured(&captured);
        let preflight = run_cats_delete(&ctx, "mainnet", Some(cat_id), None, false, false, true)
            .await
            .expect("cats-delete preflight");
        assert_eq!(preflight, 0);
        let _ = pop_captured(&captured);
        assert_eq!(
            cats_list_payload(&ctx, &captured)
                .get("cats")
                .and_then(|v| v.as_array())
                .map_or(0, std::vec::Vec::len),
            1
        );
    }
}
