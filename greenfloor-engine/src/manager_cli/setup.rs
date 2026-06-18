//! Setup, validation, and health commands for the manager CLI.

use std::path::{Path, PathBuf};

use serde_json::json;
use serde_yaml::Value;

use crate::config::{load_markets_config_with_overlay, load_program_config, require_signer_offer_path};
use crate::error::{SignerError, SignerResult};
use crate::storage::{resolve_state_db_path, SqliteStore};

use super::json::ManagerOutput;
use super::paths::expand_home;

const ALLOWED_LOG_LEVELS: &[&str] = &["CRITICAL", "ERROR", "WARNING", "INFO", "DEBUG", "NOTSET"];

pub fn run_config_validate(
    output: &ManagerOutput,
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
) -> SignerResult<i32> {
    let _program = load_program_config(program_path)?;
    let _markets = load_markets_config_with_overlay(markets_path, testnet_markets_path)?;
    output.emit_json(&json!({
        "ok": true,
        "program_config": program_path.display().to_string(),
        "markets_config": markets_path.display().to_string(),
    }))?;
    Ok(0)
}

pub fn run_set_log_level(
    output: &ManagerOutput,
    program_path: &Path,
    log_level: &str,
) -> SignerResult<i32> {
    let level = normalize_log_level(log_level)?;
    let raw = read_yaml_mapping(program_path)?;
    let mut root = raw;
    let app = root
        .as_mapping_mut()
        .ok_or_else(|| SignerError::Other("program config root must be a mapping".to_string()))?;
    let app_entry = app
        .entry(Value::from("app"))
        .or_insert_with(|| Value::Mapping(Default::default()));
    let app_map = app_entry
        .as_mapping_mut()
        .ok_or_else(|| SignerError::Other("program config field 'app' must be a mapping".to_string()))?;
    let prior_level = app_map
        .get(Value::from("log_level"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_ascii_uppercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "INFO".to_string());
    app_map.insert(Value::from("log_level"), Value::from(level.as_str()));
    write_yaml(program_path, &root)?;
    output.emit_json(&json!({
        "updated": true,
        "program_config": program_path.display().to_string(),
        "previous_log_level": prior_level,
        "log_level": level,
    }))?;
    Ok(0)
}

pub fn run_bootstrap_home(
    output: &ManagerOutput,
    home_dir: &Path,
    program_template: &Path,
    markets_template: &Path,
    cats_template: Option<&Path>,
    testnet_markets_template: Option<&Path>,
    seed_testnet_markets: bool,
    force: bool,
) -> SignerResult<i32> {
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
    store.add_audit_event(
        "home_bootstrap",
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

    output.emit_json(&json!({
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

pub fn run_doctor(
    output: &ManagerOutput,
    program_path: &Path,
    markets_path: &Path,
    state_db: Option<&str>,
    testnet_markets_path: Option<&Path>,
) -> SignerResult<i32> {
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
            problems.push(format!("market_key_error:{}:missing signer_key_id", market.market_id));
        } else {
            key_ids.push(key_id.to_string());
        }
    }
    let db_path = resolve_state_db_path(&program.home_dir, state_db);
    match SqliteStore::open(&db_path) {
        Ok(store) => {
            if let Err(err) = store.add_audit_event("doctor_ping", &json!({"ok": true}), None) {
                problems.push(format!("db_error:{err}"));
            }
        }
        Err(err) => problems.push(format!("db_error:{err}")),
    }
    if require_signer_offer_path(program_path).is_err() {
        warnings.push("signer_not_configured:kms_key_id_or_vault_launcher_id".to_string());
    }
    collect_env_warnings(&mut warnings);
    let mut resolved_key_ids: Vec<_> = key_ids.into_iter().collect();
    resolved_key_ids.sort();
    resolved_key_ids.dedup();
    let ok = problems.is_empty();
    output.emit_json(&json!({
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
        let raw = std::env::var(name).unwrap_or_default();
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

fn normalize_log_level(log_level: &str) -> SignerResult<String> {
    let level = log_level.trim().to_ascii_uppercase();
    if ALLOWED_LOG_LEVELS.contains(&level.as_str()) {
        Ok(level)
    } else {
        Err(SignerError::Other(format!(
            "log level must be one of: {}",
            ALLOWED_LOG_LEVELS.join(", ")
        )))
    }
}

fn read_yaml_mapping(path: &Path) -> SignerResult<Value> {
    let raw = std::fs::read_to_string(path).map_err(|err| {
        SignerError::Other(format!("failed to read {}: {err}", path.display()))
    })?;
    serde_yaml::from_str(&raw)
        .map_err(|err| SignerError::Other(format!("failed to parse {}: {err}", path.display())))
}

fn write_yaml(path: &Path, value: &Value) -> SignerResult<()> {
    let text = serde_yaml::to_string(value)
        .map_err(|err| SignerError::Other(format!("failed to encode yaml: {err}")))?;
    std::fs::write(path, text).map_err(|err| {
        SignerError::Other(format!("failed to write {}: {err}", path.display()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_log_level_accepts_info() {
        assert_eq!(normalize_log_level("info").expect("level"), "INFO");
    }

    #[test]
    fn normalize_log_level_rejects_garbage() {
        assert!(normalize_log_level("verbose").is_err());
    }

    #[test]
    fn config_validate_emits_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program_path = dir.path().join("program.yaml");
        let markets_path = dir.path().join("markets.yaml");
        std::fs::write(
            &program_path,
            "app:\n  network: mainnet\n  home_dir: /tmp/gf\n",
        )
        .expect("write program");
        std::fs::write(&markets_path, "markets: []\n").expect("write markets");
        let output = super::super::json::ManagerOutput::new(false);
        let code = run_config_validate(&output, &program_path, &markets_path, None)
            .expect("validate");
        assert_eq!(code, 0);
    }
}
