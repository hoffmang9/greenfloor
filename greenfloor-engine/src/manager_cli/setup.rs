//! Setup, validation, and health commands for the manager CLI.

use std::path::Path;

use serde_json::json;
use serde_yaml::Value;

use crate::config::{load_markets_config_with_overlay, load_program_config};
use crate::error::{SignerError, SignerResult};
use crate::file_logging::validate_log_level;
use crate::hex::{is_hex_id, normalize_hex_id};
use crate::minimal_program_template::{
    write_minimal_program, write_minimal_program_with_signer, MinimalProgramParams,
};
use crate::operator_log::{LogContext, DOCTOR_PING, HOME_BOOTSTRAP};
use crate::storage::{resolve_state_db_path, SqliteStore};

use super::cats_catalog::load_cats_catalog;
use super::context::ManagerContext;
use super::paths::expand_home;

pub struct BootstrapHomeParams<'a> {
    pub ctx: &'a ManagerContext,
    pub home_dir: &'a Path,
    pub program_template: &'a Path,
    pub markets_template: &'a Path,
    pub cats_template: Option<&'a Path>,
    pub testnet_markets_template: Option<&'a Path>,
    pub seed_testnet_markets: bool,
    pub force: bool,
}

pub fn validate_config(ctx: &ManagerContext, program_only: bool) -> SignerResult<()> {
    let _program = load_program_config(&ctx.program_config)?;
    if program_only {
        return Ok(());
    }
    let _markets =
        load_markets_config_with_overlay(&ctx.markets_config, ctx.testnet_markets_path())?;
    Ok(())
}

pub fn run_config_validate(ctx: &ManagerContext, program_only: bool) -> SignerResult<i32> {
    validate_config(ctx, program_only)?;
    let program_path = &ctx.program_config;
    if program_only {
        ctx.emit_json(&json!({
            "ok": true,
            "program_config": program_path.display().to_string(),
        }))?;
        return Ok(0);
    }
    let markets_path = &ctx.markets_config;
    ctx.emit_json(&json!({
        "ok": true,
        "program_config": program_path.display().to_string(),
        "markets_config": markets_path.display().to_string(),
    }))?;
    Ok(0)
}

pub fn run_program_fields(ctx: &ManagerContext) -> SignerResult<i32> {
    let program = load_program_config(&ctx.program_config)?;
    let mut keys_registry = serde_json::Map::new();
    for (key_id, entry) in &program.signer_key_registry {
        keys_registry.insert(
            key_id.clone(),
            json!({
                "key_id": key_id,
                "fingerprint": entry.fingerprint,
                "network": entry.network,
                "keyring_yaml_path": entry.keyring_yaml_path,
            }),
        );
    }
    ctx.emit_json(&json!({
        "network": program.network,
        "home_dir": program.home_dir.display().to_string(),
        "signer_kms_key_id": program.signer_kms_key_id,
        "signer_kms_region": program.signer_kms_region,
        "vault_launcher_id": program.vault_launcher_id,
        "signer_offer_path_configured": program.signer_offer_path_configured(),
        "dev_python_min_version": program.dev_python_min_version,
        "keys_registry": keys_registry,
    }))?;
    Ok(0)
}

pub fn run_markets_fields(ctx: &ManagerContext) -> SignerResult<i32> {
    let markets =
        load_markets_config_with_overlay(&ctx.markets_config, ctx.testnet_markets_path())?;
    let all: Vec<_> = markets.markets.iter().map(market_fields_row).collect();
    let enabled: Vec<_> = markets
        .markets
        .iter()
        .filter(|market| market.enabled)
        .map(market_fields_row)
        .collect();
    ctx.emit_json(&json!({
        "markets_config": ctx.markets_config.display().to_string(),
        "markets": all,
        "enabled_markets": enabled,
    }))?;
    Ok(0)
}

fn market_fields_row(market: &crate::config::MarketConfig) -> serde_json::Value {
    json!({
        "id": market.market_id,
        "enabled": market.enabled,
        "base_asset": market.base_asset,
        "base_symbol": market.base_symbol,
        "quote_asset": market.quote_asset,
        "quote_asset_type": market.quote_asset_type,
        "receive_address": market.receive_address,
        "signer_key_id": market.signer_key_id,
        "mode": market.mode,
    })
}

#[derive(Clone, Copy)]
pub struct MaterializeMinimalProgramFeatureFlags {
    pub dry_run: bool,
    pub low_inventory_alerts_enabled: bool,
    pub pushover_enabled: bool,
}

#[derive(Clone, Copy)]
pub struct MaterializeMinimalProgramRequest<'a> {
    pub output: &'a Path,
    pub home_dir: &'a Path,
    pub dexie_api_base: &'a str,
    pub log_level: &'a str,
    pub features: MaterializeMinimalProgramFeatureFlags,
    pub with_signer: bool,
}

pub fn run_materialize_minimal_program(request: MaterializeMinimalProgramRequest<'_>) -> i32 {
    let params = MinimalProgramParams {
        home_dir: request.home_dir,
        dexie_api_base: request.dexie_api_base,
        log_level: Some(request.log_level),
        dry_run: request.features.dry_run,
        low_inventory_alerts_enabled: request.features.low_inventory_alerts_enabled,
        pushover_enabled: request.features.pushover_enabled,
    };
    if request.with_signer {
        write_minimal_program_with_signer(request.output, params);
    } else {
        write_minimal_program(request.output, params);
    }
    0
}

pub fn run_cats_fields(ctx: &ManagerContext) -> SignerResult<i32> {
    let catalog = load_cats_catalog(&ctx.cats_config)?;
    let mut symbol_to_asset_id = serde_json::Map::new();
    for row in &catalog {
        let Some(symbol) = row
            .get("base_symbol")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let Some(asset_id) = row
            .get("asset_id")
            .and_then(serde_json::Value::as_str)
            .map(normalize_hex_id)
            .filter(|value| is_hex_id(value))
        else {
            continue;
        };
        symbol_to_asset_id.insert(symbol.to_ascii_lowercase(), json!(asset_id));
    }
    ctx.emit_json(&json!({
        "cats_config": ctx.cats_config.display().to_string(),
        "symbol_to_asset_id": symbol_to_asset_id,
        "cats": catalog,
    }))?;
    Ok(0)
}

pub fn run_set_log_level(ctx: &ManagerContext, log_level: &str) -> SignerResult<i32> {
    let program_path = &ctx.program_config;
    let level = validate_log_level(log_level)?;
    let raw = read_yaml_mapping(program_path)?;
    let mut root = raw;
    let app = root
        .as_mapping_mut()
        .ok_or_else(|| SignerError::Other("program config root must be a mapping".to_string()))?;
    let app_entry = app
        .entry(Value::from("app"))
        .or_insert_with(|| Value::Mapping(serde_yaml::Mapping::default()));
    let app_map = app_entry.as_mapping_mut().ok_or_else(|| {
        SignerError::Other("program config field 'app' must be a mapping".to_string())
    })?;
    let prior_level = app_map
        .get(Value::from("log_level"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_ascii_uppercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "INFO".to_string());
    app_map.insert(Value::from("log_level"), Value::from(level.as_str()));
    write_yaml(program_path, &root)?;
    ctx.emit_json(&json!({
        "updated": true,
        "program_config": program_path.display().to_string(),
        "previous_log_level": prior_level,
        "log_level": level,
    }))?;
    Ok(0)
}

pub fn run_bootstrap_home(params: &BootstrapHomeParams<'_>) -> SignerResult<i32> {
    let BootstrapHomeParams {
        ctx,
        home_dir,
        program_template,
        markets_template,
        cats_template,
        testnet_markets_template,
        seed_testnet_markets,
        force,
    } = *params;
    let home = expand_home(home_dir);
    let config_dir = home.join("config");
    let db_dir = home.join("db");
    let state_dir = home.join("state");
    let logs_dir = home.join("logs");
    for dir in [&home, &config_dir, &db_dir, &state_dir, &logs_dir] {
        std::fs::create_dir_all(dir).map_err(|err| {
            SignerError::Other(format!("failed to create {}: {err}", dir.display()))
        })?;
    }

    let seeded_program = config_dir.join("program.yaml");
    let seeded_markets = config_dir.join("markets.yaml");
    let seeded_cats = config_dir.join("cats.yaml");
    let seeded_testnet_markets = config_dir.join("testnet-markets.yaml");

    let mut wrote_program = false;
    if force || !seeded_program.exists() {
        let mut program_data = read_yaml_mapping(program_template)?;
        if let Some(app) = program_data.get_mut("app") {
            if let Some(app_map) = app.as_mapping_mut() {
                app_map.insert(
                    Value::from("home_dir"),
                    Value::from(home.display().to_string()),
                );
            }
        }
        write_yaml(&seeded_program, &program_data)?;
        wrote_program = true;
    }

    let mut wrote_markets = false;
    if force || !seeded_markets.exists() {
        let markets_data = read_yaml_mapping(markets_template)?;
        write_yaml(&seeded_markets, &markets_data)?;
        wrote_markets = true;
    }

    let mut wrote_cats = false;
    if let Some(template) = cats_template {
        if force || !seeded_cats.exists() {
            let cats_data = read_yaml_mapping(template)?;
            write_yaml(&seeded_cats, &cats_data)?;
            wrote_cats = true;
        }
    }

    let mut wrote_testnet_markets = false;
    if seed_testnet_markets {
        if let Some(template) = testnet_markets_template {
            if force || !seeded_testnet_markets.exists() {
                let data = read_yaml_mapping(template)?;
                write_yaml(&seeded_testnet_markets, &data)?;
                wrote_testnet_markets = true;
            }
        }
    }

    let db_path = db_dir.join("greenfloor.sqlite");
    let store = SqliteStore::open(&db_path)?;
    LogContext::VALIDATION.audit(
        &store,
        HOME_BOOTSTRAP,
        &json!({
            "home_dir": home.display().to_string(),
            "program_config": seeded_program.display().to_string(),
            "markets_config": seeded_markets.display().to_string(),
            "cats_config": seeded_cats.display().to_string(),
            "testnet_markets_config": seeded_testnet_markets.display().to_string(),
            "force": force,
        }),
        None,
    )?;

    ctx.emit_json(&json!({
        "bootstrapped": true,
        "home_dir": home.display().to_string(),
        "program_config": seeded_program.display().to_string(),
        "markets_config": seeded_markets.display().to_string(),
        "cats_config": seeded_cats.display().to_string(),
        "testnet_markets_config": if seed_testnet_markets {
            seeded_testnet_markets.display().to_string()
        } else {
            String::new()
        },
        "state_db": db_path.display().to_string(),
        "state_dir": state_dir.display().to_string(),
        "logs_dir": logs_dir.display().to_string(),
        "wrote_program_config": wrote_program,
        "wrote_markets_config": wrote_markets,
        "wrote_cats_config": wrote_cats,
        "wrote_testnet_markets_config": wrote_testnet_markets,
    }))?;
    Ok(0)
}

pub fn run_doctor(ctx: &ManagerContext) -> SignerResult<i32> {
    let program_path = &ctx.program_config;
    let markets_path = &ctx.markets_config;
    let state_db = ctx.state_db_override();
    let testnet_markets_path = ctx.testnet_markets_path();
    let program = load_program_config(program_path)?;
    let markets = load_markets_config_with_overlay(markets_path, testnet_markets_path)?;
    let mut problems = Vec::new();
    let mut warnings = Vec::new();
    let enabled_markets: Vec<_> = markets.markets.iter().filter(|m| m.enabled).collect();
    if enabled_markets.is_empty() {
        warnings.push("no_enabled_markets".to_string());
    }
    let mut key_ids = Vec::new();
    for market in &enabled_markets {
        let key_id = market.signer_key_id.trim();
        if key_id.is_empty() {
            problems.push(format!(
                "market_key_error:{}:missing signer_key_id",
                market.market_id
            ));
        } else {
            key_ids.push(key_id.to_string());
        }
    }
    let db_path = resolve_state_db_path(&program.home_dir, state_db);
    match SqliteStore::open(&db_path) {
        Ok(store) => {
            if let Err(err) =
                LogContext::VALIDATION.audit(&store, DOCTOR_PING, &json!({"ok": true}), None)
            {
                problems.push(format!("db_error:{err}"));
            }
        }
        Err(err) => problems.push(format!("db_error:{err}")),
    }
    if !program.signer_offer_path_configured() {
        warnings.push("signer_not_configured:kms_key_id_or_vault_launcher_id".to_string());
    }
    collect_env_warnings(&mut warnings);
    let mut resolved_key_ids: Vec<_> = key_ids.into_iter().collect();
    resolved_key_ids.sort();
    resolved_key_ids.dedup();
    let ok = problems.is_empty();
    ctx.emit_json(&json!({
        "ok": ok,
        "program_config": program_path.display().to_string(),
        "markets_config": markets_path.display().to_string(),
        "state_db": db_path.display().to_string(),
        "enabled_markets": enabled_markets.len(),
        "resolved_key_ids": resolved_key_ids,
        "warnings": warnings,
        "problems": problems,
    }))?;
    Ok(if ok { 0 } else { 2 })
}

fn collect_env_warnings(warnings: &mut Vec<String>) {
    for (name, minimum) in [
        ("GREENFLOOR_UNSTABLE_CANCEL_MOVE_BPS", 1_i64),
        ("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", 1),
        ("GREENFLOOR_OFFER_POST_BACKOFF_MS", 0),
        ("GREENFLOOR_OFFER_POST_COOLDOWN_SECONDS", 0),
        ("GREENFLOOR_OFFER_CANCEL_MAX_ATTEMPTS", 1),
        ("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS", 0),
        ("GREENFLOOR_OFFER_CANCEL_COOLDOWN_SECONDS", 0),
    ] {
        let raw = runtime_override_value(name);
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        match trimmed.parse::<i64>() {
            Ok(value) if value < minimum => {
                warnings.push(format!("invalid_env_override:{name}:must_be>={minimum}"));
            }
            Err(_) => warnings.push(format!("invalid_env_override:{name}:not_integer")),
            _ => {}
        }
    }
}

fn runtime_override_value(name: &str) -> String {
    #[cfg(test)]
    if let Some(value) = test_runtime_override_value(name) {
        return value;
    }
    std::env::var(name).unwrap_or_default()
}

#[cfg(test)]
mod test_runtime_overrides {
    use std::collections::HashMap;
    use std::sync::Mutex;

    pub static OVERRIDES: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
}

#[cfg(test)]
fn test_runtime_override_value(name: &str) -> Option<String> {
    let guard = test_runtime_overrides::OVERRIDES.lock().ok()?;
    guard.as_ref()?.get(name).cloned()
}

fn read_yaml_mapping(path: &Path) -> SignerResult<Value> {
    let raw = std::fs::read_to_string(path)
        .map_err(|err| SignerError::Other(format!("failed to read {}: {err}", path.display())))?;
    serde_yaml::from_str(&raw)
        .map_err(|err| SignerError::Other(format!("failed to parse {}: {err}", path.display())))
}

fn write_yaml(path: &Path, value: &Value) -> SignerResult<()> {
    let text = serde_yaml::to_string(value)
        .map_err(|err| SignerError::Other(format!("failed to encode yaml: {err}")))?;
    std::fs::write(path, text)
        .map_err(|err| SignerError::Other(format!("failed to write {}: {err}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_log_level_accepts_info() {
        assert_eq!(validate_log_level("info").expect("level"), "INFO");
    }

    #[test]
    fn validate_log_level_rejects_garbage() {
        assert!(validate_log_level("verbose").is_err());
    }

    #[test]
    fn config_validate_emits_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program_path = dir.path().join("program.yaml");
        let markets_path = dir.path().join("markets.yaml");
        std::fs::write(
            &program_path,
            r#"app:
  network: mainnet
  home_dir: /tmp/gf
runtime:
  loop_interval_seconds: 30
chain_signals:
  tx_block_trigger:
    mode: websocket
dev:
  python:
    min_version: "3.11"
notifications:
  low_inventory_alerts:
    enabled: true
    threshold_mode: absolute_base_units
    default_threshold_base_units: 0
    dedup_cooldown_seconds: 21600
    clear_hysteresis_percent: 10
  providers:
    - type: pushover
      enabled: true
      user_key_env: PUSHOVER_USER_KEY
      app_token_env: PUSHOVER_APP_TOKEN
      recipient_key_env: PUSHOVER_RECIPIENT_KEY
"#,
        )
        .expect("write program");
        std::fs::write(&markets_path, "markets: []\n").expect("write markets");
        let output = super::super::context::ManagerContext::for_test(program_path, markets_path);
        let code = run_config_validate(&output, false).expect("validate");
        assert_eq!(code, 0);
    }

    fn repo_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf()
    }

    #[test]
    fn program_fields_reads_example_program() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("program.yaml");
        std::fs::copy(repo_root().join("config/program.yaml"), &program).expect("copy program");
        let (output, captured) = super::super::json::ManagerOutput::capturing(true);
        let ctx = super::super::context::ManagerContext::for_test_with_cats(
            program,
            dir.path().join("unused-markets.yaml"),
            dir.path().join("unused-cats.yaml"),
            output,
        );
        let code = run_program_fields(&ctx).expect("program-fields");
        assert_eq!(code, 0);
        let payload = captured
            .lock()
            .expect("capture lock")
            .pop()
            .expect("json emitted");
        assert_eq!(
            payload.get("network").and_then(serde_json::Value::as_str),
            Some("mainnet")
        );
        let registry = payload
            .get("keys_registry")
            .and_then(serde_json::Value::as_object)
            .expect("keys registry");
        assert!(registry.contains_key("key-main-1"));
    }

    #[test]
    fn markets_fields_reads_example_markets() {
        let (output, captured) = super::super::json::ManagerOutput::capturing(true);
        let ctx = super::super::context::ManagerContext::for_test_with_output(
            repo_root().join("config/program.yaml"),
            repo_root().join("config/markets.yaml"),
            output,
        )
        .with_testnet_markets(repo_root().join("config/testnet-markets.yaml"));
        let code = run_markets_fields(&ctx).expect("markets-fields");
        assert_eq!(code, 0);
        let payload = captured
            .lock()
            .expect("capture lock")
            .pop()
            .expect("json emitted");
        let enabled = payload
            .get("enabled_markets")
            .and_then(serde_json::Value::as_array)
            .expect("enabled markets");
        assert!(!enabled.is_empty());
        assert!(enabled.iter().all(|row| {
            row.get("enabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        }));
    }

    #[test]
    fn cats_fields_reads_example_cats() {
        let (output, captured) = super::super::json::ManagerOutput::capturing(true);
        let ctx = super::super::context::ManagerContext::for_test_with_output(
            repo_root().join("config/program.yaml"),
            repo_root().join("config/markets.yaml"),
            output,
        )
        .with_cats_config(repo_root().join("config/cats.yaml"));
        let code = run_cats_fields(&ctx).expect("cats-fields");
        assert_eq!(code, 0);
        let payload = captured
            .lock()
            .expect("capture lock")
            .pop()
            .expect("json emitted");
        let symbol_map = payload
            .get("symbol_to_asset_id")
            .and_then(serde_json::Value::as_object)
            .expect("symbol_to_asset_id map");
        assert!(!symbol_map.is_empty());
    }

    #[test]
    fn config_validate_accepts_example_configs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("program.yaml");
        let markets = dir.path().join("markets.yaml");
        std::fs::copy(repo_root().join("config/program.yaml"), &program).expect("copy program");
        std::fs::copy(repo_root().join("config/markets.yaml"), &markets).expect("copy markets");
        let (output, captured) = super::super::json::ManagerOutput::capturing(true);
        let ctx = super::super::context::ManagerContext::for_test_with_cats(
            program,
            markets,
            dir.path().join("unused-cats.yaml"),
            output,
        );
        let code = run_config_validate(&ctx, false).expect("config-validate");
        assert_eq!(code, 0);
        let payload = captured
            .lock()
            .expect("capture lock")
            .pop()
            .expect("json emitted");
        assert_eq!(
            payload.get("ok").and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn config_validate_program_only_accepts_example_program() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("program.yaml");
        std::fs::copy(repo_root().join("config/program.yaml"), &program).expect("copy program");
        let ctx = super::super::context::ManagerContext::for_test(
            program,
            dir.path().join("unused-markets.yaml"),
        );
        let code = run_config_validate(&ctx, true).expect("config-validate program-only");
        assert_eq!(code, 0);
    }

    #[test]
    fn materialize_minimal_program_template_writes_expected_fields() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        let program = dir.path().join("program.yaml");
        let code = run_materialize_minimal_program(MaterializeMinimalProgramRequest {
            output: &program,
            home_dir: &home,
            dexie_api_base: "https://dexie.test",
            log_level: "INFO",
            features: MaterializeMinimalProgramFeatureFlags {
                dry_run: false,
                low_inventory_alerts_enabled: true,
                pushover_enabled: true,
            },
            with_signer: false,
        });
        assert_eq!(code, 0);
        let raw: serde_json::Value =
            serde_yaml::from_str(&std::fs::read_to_string(&program).expect("read program"))
                .expect("parse yaml");
        assert_eq!(
            raw.get("app")
                .and_then(|app| app.get("home_dir"))
                .and_then(serde_json::Value::as_str),
            Some(home.to_str().expect("home path"))
        );
        assert_eq!(
            raw.get("venues")
                .and_then(|venues| venues.get("dexie"))
                .and_then(|dexie| dexie.get("api_base"))
                .and_then(serde_json::Value::as_str),
            Some("https://dexie.test")
        );
        assert_eq!(
            raw.get("dev")
                .and_then(|dev| dev.get("python"))
                .and_then(|python| python.get("min_version"))
                .and_then(serde_json::Value::as_str),
            Some("3.11")
        );
    }

    fn write_bootstrap_templates(
        root: &std::path::Path,
    ) -> (
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
    ) {
        let program_template = root.join("program.template.yaml");
        let markets_template = root.join("markets.template.yaml");
        let cats_template = root.join("cats.template.yaml");
        let testnet_markets_template = root.join("testnet-markets.template.yaml");
        std::fs::write(
            &program_template,
            r#"app:
  network: "mainnet"
  home_dir: "~/.greenfloor"
runtime:
  loop_interval_seconds: 30
notifications:
  low_inventory_alerts:
    enabled: true
    threshold_mode: "absolute_base_units"
    default_threshold_base_units: 0
    dedup_cooldown_seconds: 3600
    clear_hysteresis_percent: 10
  providers:
    - type: pushover
      enabled: false
      user_key_env: "PUSHOVER_USER_KEY"
      app_token_env: "PUSHOVER_APP_TOKEN"
      recipient_key_env: "PUSHOVER_RECIPIENT_KEY"
chain_signals:
  tx_block_trigger:
    webhook_enabled: true
    webhook_listen_addr: "127.0.0.1:8787"
"#,
        )
        .expect("write program template");
        std::fs::write(
            &markets_template,
            r#"markets:
  - id: m1
    enabled: true
    base_asset: "a1"
    base_symbol: "A1"
    quote_asset: "xch"
    quote_asset_type: "unstable"
    signer_key_id: "key-main-1"
    receive_address: "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
    inventory:
      low_watermark_base_units: 100
"#,
        )
        .expect("write markets template");
        std::fs::write(
            &cats_template,
            r#"cats:
  - name: Token One
    base_symbol: "TOK1"
    asset_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    target_usd_per_unit: null
    dexie:
      ticker_id: null
      pool_id: null
      last_price_xch: null
"#,
        )
        .expect("write cats template");
        std::fs::write(
            &testnet_markets_template,
            r#"markets:
  - id: m-testnet
    enabled: true
    base_asset: "ta1"
    base_symbol: "TA1"
    quote_asset: "txch"
    quote_asset_type: "unstable"
    signer_key_id: "key-main-1"
    receive_address: "txch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h"
    mode: "sell_only"
    inventory:
      low_watermark_base_units: 100
"#,
        )
        .expect("write testnet template");
        (
            program_template,
            markets_template,
            cats_template,
            testnet_markets_template,
        )
    }

    fn bootstrap_home_in_process(
        home_dir: &std::path::Path,
        program_template: &std::path::Path,
        markets_template: &std::path::Path,
        cats_template: &std::path::Path,
        testnet_markets_template: &std::path::Path,
        seed_testnet_markets: bool,
        force: bool,
    ) -> i32 {
        let ctx = super::super::context::ManagerContext::for_test(
            program_template.to_path_buf(),
            markets_template.to_path_buf(),
        );
        run_bootstrap_home(&BootstrapHomeParams {
            ctx: &ctx,
            home_dir,
            program_template,
            markets_template,
            cats_template: Some(cats_template),
            testnet_markets_template: Some(testnet_markets_template),
            seed_testnet_markets,
            force,
        })
        .expect("bootstrap-home")
    }

    #[test]
    fn bootstrap_home_creates_layout_and_seed_configs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home_dir = dir.path().join(".greenfloor");
        let (program_template, markets_template, cats_template, testnet_markets_template) =
            write_bootstrap_templates(dir.path());
        assert_eq!(
            bootstrap_home_in_process(
                &home_dir,
                &program_template,
                &markets_template,
                &cats_template,
                &testnet_markets_template,
                false,
                false,
            ),
            0
        );
        assert!(home_dir.join("config").is_dir());
        assert!(home_dir.join("db").is_dir());
        assert!(home_dir.join("state").is_dir());
        assert!(home_dir.join("logs").is_dir());
        assert!(home_dir.join("db").join("greenfloor.sqlite").is_file());
        assert!(home_dir.join("config").join("program.yaml").is_file());
        assert!(home_dir.join("config").join("markets.yaml").is_file());
        assert!(home_dir.join("config").join("cats.yaml").is_file());
    }

    #[test]
    fn bootstrap_home_without_force_keeps_existing_seeded_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home_dir = dir.path().join(".greenfloor");
        let config_dir = home_dir.join("config");
        std::fs::create_dir_all(&config_dir).expect("create config");
        std::fs::write(
            config_dir.join("program.yaml"),
            "app:\n  home_dir: \"custom-home\"\n",
        )
        .expect("write program");
        std::fs::write(config_dir.join("markets.yaml"), "markets: []\n").expect("write markets");
        std::fs::write(config_dir.join("cats.yaml"), "cats: []\n").expect("write cats");
        let (program_template, markets_template, cats_template, testnet_markets_template) =
            write_bootstrap_templates(dir.path());
        assert_eq!(
            bootstrap_home_in_process(
                &home_dir,
                &program_template,
                &markets_template,
                &cats_template,
                &testnet_markets_template,
                false,
                false,
            ),
            0
        );
        assert_eq!(
            std::fs::read_to_string(config_dir.join("program.yaml")).expect("read program"),
            "app:\n  home_dir: \"custom-home\"\n"
        );
    }

    #[test]
    fn bootstrap_home_can_seed_optional_testnet_markets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home_dir = dir.path().join(".greenfloor");
        let (program_template, markets_template, cats_template, testnet_markets_template) =
            write_bootstrap_templates(dir.path());
        assert_eq!(
            bootstrap_home_in_process(
                &home_dir,
                &program_template,
                &markets_template,
                &cats_template,
                &testnet_markets_template,
                true,
                false,
            ),
            0
        );
        assert!(home_dir
            .join("config")
            .join("testnet-markets.yaml")
            .is_file());
    }

    fn copy_example_program_and_markets(
        dir: &std::path::Path,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        let program = dir.join("program.yaml");
        let markets = dir.join("markets.yaml");
        std::fs::copy(repo_root().join("config/program.yaml"), &program).expect("copy program");
        std::fs::copy(repo_root().join("config/markets.yaml"), &markets).expect("copy markets");
        (program, markets)
    }

    fn pop_captured(
        captured: &std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
    ) -> serde_json::Value {
        captured
            .lock()
            .expect("capture lock")
            .pop()
            .expect("json emitted")
    }

    fn doctor_context(
        program: std::path::PathBuf,
        markets: std::path::PathBuf,
        state_db: &str,
    ) -> (
        super::super::context::ManagerContext,
        std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
    ) {
        let (output, captured) = super::super::json::ManagerOutput::capturing(true);
        let mut ctx =
            super::super::context::ManagerContext::for_test_with_output(program, markets, output);
        ctx.state_db = state_db.to_string();
        (ctx, captured)
    }

    #[cfg(test)]
    struct TestRuntimeOverrides {
        _private: (),
    }

    #[cfg(test)]
    impl TestRuntimeOverrides {
        fn new(values: &[(&str, &str)]) -> Self {
            use std::collections::HashMap;
            let mut map = HashMap::new();
            for (name, value) in values {
                map.insert((*name).to_string(), (*value).to_string());
            }
            *test_runtime_overrides::OVERRIDES
                .lock()
                .expect("override lock") = Some(map);
            Self { _private: () }
        }
    }

    #[cfg(test)]
    impl Drop for TestRuntimeOverrides {
        fn drop(&mut self) {
            *test_runtime_overrides::OVERRIDES
                .lock()
                .expect("override lock") = None;
        }
    }

    #[test]
    fn doctor_reports_ok_with_example_configs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (program, markets) = copy_example_program_and_markets(dir.path());
        let state_db = dir.path().join("state.sqlite");
        let (ctx, _captured) =
            doctor_context(program, markets, state_db.to_str().expect("state db"));
        let code = run_doctor(&ctx).expect("doctor");
        assert_eq!(code, 0);
    }

    #[test]
    fn doctor_fails_when_enabled_market_key_missing_from_registry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (program, markets) = copy_example_program_and_markets(dir.path());
        let markets_text = std::fs::read_to_string(&markets).expect("read markets");
        let patched = markets_text.replace("signer_key_id:", "signer_key_id: \"\" #");
        std::fs::write(&markets, patched).expect("patch markets");
        let state_db = dir.path().join("state.sqlite");
        let (ctx, captured) =
            doctor_context(program, markets, state_db.to_str().expect("state db"));
        let code = run_doctor(&ctx).expect("doctor");
        assert_eq!(code, 2);
        let payload = pop_captured(&captured);
        assert_eq!(payload.get("ok"), Some(&serde_json::json!(false)));
        let problems = payload
            .get("problems")
            .and_then(|v| v.as_array())
            .expect("problems");
        assert!(problems.iter().any(|problem| {
            problem
                .as_str()
                .is_some_and(|text| text.contains("missing signer_key_id"))
        }));
    }

    #[test]
    fn doctor_warns_on_invalid_runtime_override_env() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (program, markets) = copy_example_program_and_markets(dir.path());
        let state_db = dir.path().join("state.sqlite");
        let _overrides = TestRuntimeOverrides::new(&[
            ("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS", "0"),
            ("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS", "bad"),
        ]);
        let (ctx, captured) =
            doctor_context(program, markets, state_db.to_str().expect("state db"));
        let code = run_doctor(&ctx).expect("doctor");
        assert_eq!(code, 0);
        let payload = pop_captured(&captured);
        let warnings = payload
            .get("warnings")
            .and_then(|v| v.as_array())
            .expect("warnings");
        assert!(warnings.iter().any(|warning| {
            warning
                .as_str()
                .is_some_and(|text| text.contains("GREENFLOOR_OFFER_POST_MAX_ATTEMPTS"))
        }));
        assert!(warnings.iter().any(|warning| {
            warning
                .as_str()
                .is_some_and(|text| text.contains("GREENFLOOR_OFFER_CANCEL_BACKOFF_MS"))
        }));
    }
}
