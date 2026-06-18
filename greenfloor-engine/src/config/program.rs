use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::program_validate::validate_program_config;
use crate::coinset::is_xch_like_asset;
use crate::error::{SignerError, SignerResult};
use crate::hex::is_hex_id;

const DEFAULT_DEXIE_API_BASE: &str = "https://api.dexie.space";
const DEFAULT_SPLASH_API_BASE: &str = "http://john-deere.hoffmang.com:4000";
const DEFAULT_HOME_DIR: &str = "~/.greenfloor";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerKeyEntry {
    pub key_id: String,
    pub fingerprint: u64,
    pub network: Option<String>,
    pub keyring_yaml_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ManagerProgramConfig {
    pub network: String,
    pub home_dir: PathBuf,
    pub app_log_level: String,
    pub app_log_level_was_missing: bool,
    pub dexie_api_base: String,
    pub splash_api_base: String,
    pub offer_publish_venue: String,
    pub coin_ops_minimum_fee_mojos: u64,
    pub coin_ops_max_operations_per_run: i64,
    pub coin_ops_max_daily_fee_budget_mojos: i64,
    pub coin_ops_split_fee_mojos: i64,
    pub coin_ops_combine_fee_mojos: i64,
    pub runtime_offer_bootstrap_wait_timeout_seconds: u64,
    pub runtime_market_slot_count: u64,
    pub runtime_offer_parallelism_enabled: bool,
    pub runtime_offer_parallelism_max_workers: usize,
    pub runtime_reservation_ttl_seconds: u64,
    pub runtime_dry_run: bool,
    pub runtime_loop_interval_seconds: u64,
    pub tx_block_trigger_mode: String,
    pub tx_block_websocket_url: String,
    pub tx_block_websocket_reconnect_interval_seconds: u64,
    pub tx_block_fallback_poll_interval_seconds: u64,
    pub signer_key_registry: HashMap<String, SignerKeyEntry>,
}

impl Default for ManagerProgramConfig {
    fn default() -> Self {
        Self {
            network: "mainnet".to_string(),
            home_dir: expand_home_dir(DEFAULT_HOME_DIR),
            app_log_level: "INFO".to_string(),
            app_log_level_was_missing: true,
            dexie_api_base: DEFAULT_DEXIE_API_BASE.to_string(),
            splash_api_base: DEFAULT_SPLASH_API_BASE.to_string(),
            offer_publish_venue: "dexie".to_string(),
            coin_ops_minimum_fee_mojos: 10_000_000,
            coin_ops_max_operations_per_run: 20,
            coin_ops_max_daily_fee_budget_mojos: 0,
            coin_ops_split_fee_mojos: 0,
            coin_ops_combine_fee_mojos: 0,
            runtime_offer_bootstrap_wait_timeout_seconds: 120,
            runtime_market_slot_count: 0,
            runtime_offer_parallelism_enabled: false,
            runtime_offer_parallelism_max_workers: 4,
            runtime_reservation_ttl_seconds: 300,
            runtime_dry_run: false,
            runtime_loop_interval_seconds: 30,
            tx_block_trigger_mode: "websocket".to_string(),
            tx_block_websocket_url: "wss://api.coinset.org/ws".to_string(),
            tx_block_websocket_reconnect_interval_seconds: 30,
            tx_block_fallback_poll_interval_seconds: 60,
            signer_key_registry: HashMap::new(),
        }
    }
}

pub fn parse_program_config(raw: &Value) -> SignerResult<ManagerProgramConfig> {
    validate_program_config(raw)?;

    let app = req_mapping(raw, "app")?;
    let runtime = req_mapping(raw, "runtime")?;
    let chain_signals = req_mapping(raw, "chain_signals")?;
    let tx_trigger = req_value(chain_signals, "tx_block_trigger")?
        .as_object()
        .ok_or_else(|| program_err("chain_signals.tx_block_trigger must be a mapping"))?;

    let venues = raw.get("venues").and_then(Value::as_object);
    let dexie = venues
        .and_then(|section| section.get("dexie"))
        .and_then(Value::as_object);
    let splash = venues
        .and_then(|section| section.get("splash"))
        .and_then(Value::as_object);
    let offer_publish = venues
        .and_then(|section| section.get("offer_publish"))
        .and_then(Value::as_object);
    let coin_ops = raw.get("coin_ops").and_then(Value::as_object);

    let app_log_level_was_missing = !app.contains_key("log_level");
    let app_log_level = normalize_manager_log_level(
        app.get("log_level")
            .and_then(Value::as_str)
            .unwrap_or("INFO"),
    );
    let network = req_str(app, "network")?;
    let home_dir = expand_home_dir(req_str(app, "home_dir")?.trim());

    let dexie_api_base = dexie
        .and_then(|section| section.get("api_base"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_DEXIE_API_BASE)
        .trim_end_matches('/')
        .to_string();
    let splash_api_base = splash
        .and_then(|section| section.get("api_base"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_SPLASH_API_BASE)
        .trim_end_matches('/')
        .to_string();
    let offer_publish_venue = offer_publish
        .and_then(|section| section.get("provider"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("dexie")
        .to_ascii_lowercase();

    let coin_ops_minimum_fee_mojos = parse_i64_field(
        coin_ops
            .and_then(|section| section.get("minimum_fee_mojos"))
            .unwrap_or(&Value::Number(10_000_000.into())),
        "coin_ops.minimum_fee_mojos",
    )? as u64;
    let coin_ops_max_operations_per_run = parse_i64_field(
        coin_ops
            .and_then(|section| section.get("max_operations_per_run"))
            .unwrap_or(&Value::Number(20.into())),
        "coin_ops.max_operations_per_run",
    )?;
    let coin_ops_max_daily_fee_budget_mojos = parse_i64_field(
        coin_ops
            .and_then(|section| section.get("max_daily_fee_budget_mojos"))
            .unwrap_or(&Value::Number(0.into())),
        "coin_ops.max_daily_fee_budget_mojos",
    )?;
    let coin_ops_split_fee_mojos = parse_i64_field(
        coin_ops
            .and_then(|section| section.get("split_fee_mojos"))
            .unwrap_or(&Value::Number(0.into())),
        "coin_ops.split_fee_mojos",
    )?;
    let coin_ops_combine_fee_mojos = parse_i64_field(
        coin_ops
            .and_then(|section| section.get("combine_fee_mojos"))
            .unwrap_or(&Value::Number(0.into())),
        "coin_ops.combine_fee_mojos",
    )?;

    let runtime_loop_interval_seconds = parse_u64_field(
        req_value(runtime, "loop_interval_seconds")?,
        "runtime.loop_interval_seconds",
    )?;
    let runtime_dry_run = runtime
        .get("dry_run")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let runtime_market_slot_count = parse_u64_field(
        runtime
            .get("market_slot_count")
            .unwrap_or(&Value::Number(0.into())),
        "runtime.market_slot_count",
    )?;
    let runtime_offer_parallelism_enabled = runtime
        .get("offer_parallelism_enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let runtime_offer_parallelism_max_workers = parse_u64_field(
        runtime
            .get("offer_parallelism_max_workers")
            .unwrap_or(&Value::Number(4.into())),
        "runtime.offer_parallelism_max_workers",
    )?
    .max(1) as usize;
    let runtime_reservation_ttl_seconds = parse_u64_field(
        runtime
            .get("reservation_ttl_seconds")
            .unwrap_or(&Value::Number(300.into())),
        "runtime.reservation_ttl_seconds",
    )?
    .max(30);
    let runtime_offer_bootstrap_wait_timeout_seconds = runtime_timeout_seconds(
        runtime,
        "offer_bootstrap_wait_timeout_seconds",
        "cloud_wallet_bootstrap_wait_timeout_seconds",
        120,
        10,
    )?;

    let tx_block_trigger_mode = tx_trigger
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("websocket")
        .trim()
        .to_ascii_lowercase();
    let mut tx_block_websocket_url = tx_trigger
        .get("websocket_url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if tx_block_websocket_url.is_empty() {
        tx_block_websocket_url = if is_testnet_network(&network) {
            "wss://testnet11.api.coinset.org/ws".to_string()
        } else {
            "wss://api.coinset.org/ws".to_string()
        };
    }
    let tx_block_websocket_reconnect_interval_seconds = parse_u64_field(
        tx_trigger
            .get("websocket_reconnect_interval_seconds")
            .unwrap_or(&Value::Number(30.into())),
        "chain_signals.tx_block_trigger.websocket_reconnect_interval_seconds",
    )?;
    let tx_block_fallback_poll_interval_seconds = parse_u64_field(
        tx_trigger
            .get("fallback_poll_interval_seconds")
            .unwrap_or(&Value::Number(60.into())),
        "chain_signals.tx_block_trigger.fallback_poll_interval_seconds",
    )?;

    let dev = req_mapping(raw, "dev")?;
    let python = req_mapping_from_map(dev, "python")?;
    let _python_min_version = req_str(python, "min_version")?;

    let notifications = req_mapping(raw, "notifications")?;
    let _low = req_value(notifications, "low_inventory_alerts")?;
    let providers = req_value(notifications, "providers")?
        .as_array()
        .ok_or_else(|| program_err("notifications.providers must be a list"))?;
    let pushover = providers
        .iter()
        .find(|provider| {
            provider
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|value| value.trim() == "pushover")
        })
        .ok_or_else(|| program_err("Missing notifications.providers entry with type=pushover"))?
        .as_object()
        .ok_or_else(|| program_err("notifications.providers entry must be a mapping"))?;
    let _pushover_enabled = req_bool(pushover, "enabled")?;
    let _pushover_user_key_env = req_str(pushover, "user_key_env")?;
    let _pushover_app_token_env = req_str(pushover, "app_token_env")?;
    let _pushover_recipient_key_env = req_str(pushover, "recipient_key_env")?;

    let signer_key_registry = parse_signer_key_registry(raw)?;

    Ok(ManagerProgramConfig {
        network,
        home_dir,
        app_log_level,
        app_log_level_was_missing,
        dexie_api_base,
        splash_api_base,
        offer_publish_venue,
        coin_ops_minimum_fee_mojos,
        coin_ops_max_operations_per_run,
        coin_ops_max_daily_fee_budget_mojos,
        coin_ops_split_fee_mojos,
        coin_ops_combine_fee_mojos,
        runtime_offer_bootstrap_wait_timeout_seconds,
        runtime_market_slot_count,
        runtime_offer_parallelism_enabled,
        runtime_offer_parallelism_max_workers,
        runtime_reservation_ttl_seconds,
        runtime_dry_run,
        runtime_loop_interval_seconds,
        tx_block_trigger_mode,
        tx_block_websocket_url,
        tx_block_websocket_reconnect_interval_seconds,
        tx_block_fallback_poll_interval_seconds,
        signer_key_registry,
    })
}

pub fn load_program_config(path: &Path) -> SignerResult<ManagerProgramConfig> {
    let raw = std::fs::read_to_string(path).map_err(|err| {
        SignerError::Other(format!("failed to read config {}: {err}", path.display()))
    })?;
    let parsed: Value = serde_yaml::from_str(&raw).map_err(|err| {
        SignerError::Other(format!("failed to parse config {}: {err}", path.display()))
    })?;
    parse_program_config(&parsed)
}

pub fn require_signer_offer_path(path: &Path) -> SignerResult<()> {
    let raw = std::fs::read_to_string(path).map_err(|err| {
        SignerError::Other(format!("failed to read config {}: {err}", path.display()))
    })?;
    let parsed: Value = serde_yaml::from_str(&raw).map_err(|err| {
        SignerError::Other(format!("failed to parse config {}: {err}", path.display()))
    })?;
    let signer = parsed.get("signer").and_then(Value::as_object);
    let kms_key_id = signer
        .and_then(|section| section.get("kms_key_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let vault = parsed.get("vault").and_then(Value::as_object);
    let launcher_id = vault
        .and_then(|section| section.get("launcher_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if kms_key_id.is_empty() || launcher_id.is_empty() {
        return Err(SignerError::Other(
            "offer execution requires signer.kms_key_id and vault.launcher_id in program config"
                .to_string(),
        ));
    }
    Ok(())
}

pub fn is_testnet_network(network: &str) -> bool {
    matches!(
        network.trim().to_ascii_lowercase().as_str(),
        "testnet" | "testnet11"
    )
}

pub fn resolve_trade_asset_for_network(asset: &str, network: &str) -> String {
    let normalized = asset.trim().to_ascii_lowercase();
    if is_xch_like_asset(&normalized) {
        if is_testnet_network(network) {
            "txch".to_string()
        } else {
            "xch".to_string()
        }
    } else if is_hex_id(&normalized) {
        normalized
    } else {
        asset.trim().to_string()
    }
}

pub fn resolve_quote_asset_for_offer(quote_asset: &str, network: &str) -> String {
    resolve_trade_asset_for_network(quote_asset, network)
}

pub fn resolve_dexie_base_url(
    network: &str,
    explicit: Option<&str>,
    program_base: &str,
) -> SignerResult<String> {
    if let Some(url) = explicit.map(str::trim).filter(|value| !value.is_empty()) {
        return Ok(url.trim_end_matches('/').to_string());
    }
    if is_testnet_network(network) {
        return Ok("https://api-testnet.dexie.space".to_string());
    }
    Ok(program_base.trim().trim_end_matches('/').to_string())
}

pub fn resolve_splash_base_url(explicit: Option<&str>, program_base: &str) -> String {
    explicit
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_end_matches('/').to_string())
        .unwrap_or_else(|| program_base.trim().trim_end_matches('/').to_string())
}

pub fn resolve_offer_publish_settings(
    program: &ManagerProgramConfig,
    network: &str,
    venue_override: Option<&str>,
    dexie_base_url: Option<&str>,
    splash_base_url: Option<&str>,
) -> SignerResult<(String, String, String)> {
    let venue = venue_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_else(|| program.offer_publish_venue.clone());
    if venue != "dexie" && venue != "splash" {
        return Err(SignerError::Other(
            "offer publish venue must be dexie or splash".to_string(),
        ));
    }
    let dexie_base = resolve_dexie_base_url(network, dexie_base_url, &program.dexie_api_base)?;
    let splash_base = resolve_splash_base_url(splash_base_url, &program.splash_api_base);
    Ok((venue, dexie_base, splash_base))
}

pub fn action_side_from_pricing(pricing: &Value) -> String {
    pricing
        .get("side")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("sell")
        .to_string()
}

fn parse_signer_key_registry(raw: &Value) -> SignerResult<HashMap<String, SignerKeyEntry>> {
    let keys_root = raw.get("keys").and_then(Value::as_object);
    let registry_rows = keys_root
        .and_then(|keys| keys.get("registry"))
        .unwrap_or(&Value::Null);

    let rows: Vec<&Value> = if registry_rows.is_null() {
        Vec::new()
    } else {
        registry_rows
            .as_array()
            .ok_or_else(|| program_err("keys.registry must be a list"))?
            .iter()
            .collect()
    };

    let mut key_registry = HashMap::new();
    for row in rows {
        let row_map = row
            .as_object()
            .ok_or_else(|| program_err("keys.registry entries must be mappings"))?;
        let key_id = req_str(row_map, "key_id")?.trim().to_string();
        if key_id.is_empty() {
            return Err(program_err("keys.registry entry key_id must be non-empty"));
        }
        let fingerprint_raw = req_value(row_map, "fingerprint")?;
        let fingerprint =
            parse_i64_field(fingerprint_raw, &format!("fingerprint for key_id={key_id}"))
                .map_err(|_| program_err(format!("invalid fingerprint for key_id={key_id}")))?;
        if fingerprint <= 0 {
            return Err(program_err(format!(
                "fingerprint for key_id={key_id} must be positive"
            )));
        }
        if key_registry.contains_key(&key_id) {
            return Err(program_err(format!(
                "duplicate key_id in keys.registry: {key_id}"
            )));
        }
        let network = optional_trimmed_string(row_map.get("network"));
        let keyring_yaml_path = optional_trimmed_string(row_map.get("keyring_yaml_path"));
        key_registry.insert(
            key_id.clone(),
            SignerKeyEntry {
                key_id,
                fingerprint: fingerprint as u64,
                network,
                keyring_yaml_path,
            },
        );
    }
    Ok(key_registry)
}

fn runtime_timeout_seconds(
    runtime: &serde_json::Map<String, Value>,
    neutral_key: &str,
    legacy_key: &str,
    default: u64,
    minimum: u64,
) -> SignerResult<u64> {
    for key in [neutral_key, legacy_key] {
        if let Some(raw) = runtime.get(key) {
            let parsed = parse_u64_field(raw, key)?;
            return Ok(parsed.max(minimum));
        }
    }
    Ok(default.max(minimum))
}

fn optional_trimmed_string(raw: Option<&Value>) -> Option<String> {
    raw.and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn program_err(message: impl Into<String>) -> SignerError {
    SignerError::Other(message.into())
}

fn req_mapping<'a>(
    value: &'a Value,
    key: &str,
) -> SignerResult<&'a serde_json::Map<String, Value>> {
    match value.get(key) {
        Some(Value::Object(map)) => Ok(map),
        Some(_) => Err(program_err(format!("{key} must be a mapping"))),
        None => Err(program_err(format!("Missing required field: {key}"))),
    }
}

fn req_mapping_from_map<'a>(
    map: &'a serde_json::Map<String, Value>,
    key: &str,
) -> SignerResult<&'a serde_json::Map<String, Value>> {
    match map.get(key) {
        Some(Value::Object(nested)) => Ok(nested),
        Some(_) => Err(program_err(format!("{key} must be a mapping"))),
        None => Err(program_err(format!("Missing required field: {key}"))),
    }
}

fn req_value<'a>(map: &'a serde_json::Map<String, Value>, key: &str) -> SignerResult<&'a Value> {
    map.get(key)
        .ok_or_else(|| program_err(format!("Missing required field: {key}")))
}

fn req_str(map: &serde_json::Map<String, Value>, key: &str) -> SignerResult<String> {
    Ok(req_value(map, key)?
        .as_str()
        .ok_or_else(|| program_err(format!("Missing required field: {key}")))?
        .to_string())
}

fn req_bool(map: &serde_json::Map<String, Value>, key: &str) -> SignerResult<bool> {
    req_value(map, key)?
        .as_bool()
        .ok_or_else(|| program_err(format!("Missing required field: {key}")))
}

fn parse_i64_field(raw: &Value, context: &str) -> SignerResult<i64> {
    if let Some(value) = raw.as_i64() {
        return Ok(value);
    }
    if let Some(value) = raw.as_u64() {
        return Ok(value as i64);
    }
    if let Some(text) = raw.as_str() {
        if let Ok(value) = text.parse::<i64>() {
            return Ok(value);
        }
    }
    Err(program_err(format!("{context} must be an integer")))
}

fn parse_u64_field(raw: &Value, context: &str) -> SignerResult<u64> {
    let value = parse_i64_field(raw, context)?;
    if value < 0 {
        return Err(program_err(format!("{context} must be >= 0")));
    }
    Ok(value as u64)
}

fn normalize_manager_log_level(log_level: &str) -> String {
    match log_level.trim().to_ascii_uppercase().as_str() {
        "CRITICAL" | "ERROR" | "WARNING" | "INFO" | "DEBUG" | "NOTSET" => {
            log_level.trim().to_ascii_uppercase()
        }
        _ => "INFO".to_string(),
    }
}

fn expand_home_dir(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    }
    if path == "~" {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home);
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base_program_raw() -> Value {
        json!({
            "app": {"network": "mainnet", "home_dir": "~/.greenfloor", "log_level": "INFO"},
            "keys": {
                "registry": [{
                    "key_id": "key-main-1",
                    "fingerprint": 123456789,
                    "network": "mainnet",
                    "keyring_yaml_path": "~/.chia_keys/keyring.yaml"
                }]
            },
            "runtime": {"loop_interval_seconds": 30, "dry_run": false},
            "chain_signals": {
                "tx_block_trigger": {
                    "mode": "websocket",
                    "websocket_url": "",
                    "websocket_reconnect_interval_seconds": 30,
                    "fallback_poll_interval_seconds": 60
                }
            },
            "venues": {
                "dexie": {"api_base": "https://api.dexie.space"},
                "splash": {"api_base": "http://localhost:4000"},
                "offer_publish": {"provider": "dexie"}
            },
            "coin_ops": {"minimum_fee_mojos": 0},
            "dev": {"python": {"min_version": "3.11"}},
            "notifications": {
                "low_inventory_alerts": {
                    "enabled": true,
                    "threshold_mode": "absolute_base_units",
                    "default_threshold_base_units": 0,
                    "dedup_cooldown_seconds": 21600,
                    "clear_hysteresis_percent": 10
                },
                "providers": [{
                    "type": "pushover",
                    "enabled": true,
                    "user_key_env": "PUSHOVER_USER_KEY",
                    "app_token_env": "PUSHOVER_APP_TOKEN",
                    "recipient_key_env": "PUSHOVER_RECIPIENT_KEY"
                }]
            }
        })
    }

    fn parse_err(raw: &Value) -> String {
        parse_program_config(raw).unwrap_err().to_string()
    }

    #[test]
    fn parse_program_config_minimal_valid() {
        let cfg = parse_program_config(&base_program_raw()).expect("config");
        assert_eq!(cfg.network, "mainnet");
        assert_eq!(cfg.home_dir, expand_home_dir("~/.greenfloor"));
        assert_eq!(cfg.runtime_loop_interval_seconds, 30);
        assert!(!cfg.runtime_dry_run);
        assert_eq!(cfg.tx_block_trigger_mode, "websocket");
        assert_eq!(cfg.runtime_market_slot_count, 0);
        assert!(!cfg.runtime_offer_parallelism_enabled);
        assert_eq!(cfg.runtime_offer_parallelism_max_workers, 4);
        assert_eq!(cfg.runtime_reservation_ttl_seconds, 300);
        assert_eq!(cfg.offer_publish_venue, "dexie");
        assert_eq!(cfg.coin_ops_minimum_fee_mojos, 0);
        assert_eq!(cfg.app_log_level, "INFO");
        assert!(!cfg.app_log_level_was_missing);
        assert!(cfg.signer_key_registry.contains_key("key-main-1"));
        let reg = cfg
            .signer_key_registry
            .get("key-main-1")
            .expect("registry entry");
        assert_eq!(reg.fingerprint, 123456789);
        assert_eq!(reg.network.as_deref(), Some("mainnet"));
    }

    #[test]
    fn parse_program_config_websocket_url_defaults_mainnet() {
        let mut raw = base_program_raw();
        raw["chain_signals"]["tx_block_trigger"]["websocket_url"] = json!("");
        let cfg = parse_program_config(&raw).expect("config");
        assert_eq!(cfg.tx_block_websocket_url, "wss://api.coinset.org/ws");
    }

    #[test]
    fn parse_program_config_websocket_url_defaults_testnet11() {
        let mut raw = base_program_raw();
        raw["app"]["network"] = json!("testnet11");
        raw["chain_signals"]["tx_block_trigger"]["websocket_url"] = json!("");
        let cfg = parse_program_config(&raw).expect("config");
        assert_eq!(
            cfg.tx_block_websocket_url,
            "wss://testnet11.api.coinset.org/ws"
        );
    }

    #[test]
    fn parse_program_config_explicit_websocket_url_preserved() {
        let mut raw = base_program_raw();
        raw["chain_signals"]["tx_block_trigger"]["websocket_url"] =
            json!("wss://custom.example.com/ws");
        let cfg = parse_program_config(&raw).expect("config");
        assert_eq!(cfg.tx_block_websocket_url, "wss://custom.example.com/ws");
    }

    #[test]
    fn parse_program_config_rejects_cloud_wallet_block() {
        let mut raw = base_program_raw();
        raw["cloud_wallet"] = json!({
            "base_url": "https://api.vault.chia.net",
            "user_key_id": "uk-123",
            "private_key_pem_path": "/tmp/key.pem",
            "vault_id": "Wallet_abc"
        });
        let err = parse_err(&raw);
        assert!(err.contains("cloud_wallet config is removed"));
    }

    #[test]
    fn parse_program_config_rejects_cloud_wallet_mapping() {
        let mut raw = base_program_raw();
        raw["cloud_wallet"] = json!({"base_url": "https://example.com"});
        let err = parse_err(&raw);
        assert!(err.contains("cloud_wallet config is removed"));
    }

    #[test]
    fn parse_program_config_rejects_cloud_wallet_string() {
        let mut raw = base_program_raw();
        raw["cloud_wallet"] = json!("bad");
        let err = parse_err(&raw);
        assert!(err.contains("cloud_wallet config is removed"));
    }

    #[test]
    fn parse_program_config_runtime_offer_fields_default_without_legacy_keys() {
        let cfg = parse_program_config(&base_program_raw()).expect("config");
        assert_eq!(cfg.runtime_offer_bootstrap_wait_timeout_seconds, 120);
    }

    #[test]
    fn parse_program_config_log_level_missing_defaults_to_info() {
        let mut raw = base_program_raw();
        raw["app"].as_object_mut().unwrap().remove("log_level");
        let cfg = parse_program_config(&raw).expect("config");
        assert_eq!(cfg.app_log_level, "INFO");
        assert!(cfg.app_log_level_was_missing);
    }

    #[test]
    fn parse_program_config_log_level_invalid_defaults_to_info() {
        let mut raw = base_program_raw();
        raw["app"]["log_level"] = json!("VERBOSE");
        let cfg = parse_program_config(&raw).expect("config");
        assert_eq!(cfg.app_log_level, "INFO");
    }

    #[test]
    fn parse_program_config_splash_venue() {
        let mut raw = base_program_raw();
        raw["venues"]["offer_publish"]["provider"] = json!("splash");
        let cfg = parse_program_config(&raw).expect("config");
        assert_eq!(cfg.offer_publish_venue, "splash");
    }

    #[test]
    fn parse_program_config_market_slot_count() {
        let mut raw = base_program_raw();
        raw["runtime"]["market_slot_count"] = json!(4);
        let cfg = parse_program_config(&raw).expect("config");
        assert_eq!(cfg.runtime_market_slot_count, 4);
    }

    #[test]
    fn parse_program_config_offer_parallelism_runtime_flags() {
        let mut raw = base_program_raw();
        raw["runtime"]["offer_parallelism_enabled"] = json!(true);
        raw["runtime"]["offer_parallelism_max_workers"] = json!(2);
        raw["runtime"]["reservation_ttl_seconds"] = json!(900);
        let cfg = parse_program_config(&raw).expect("config");
        assert!(cfg.runtime_offer_parallelism_enabled);
        assert_eq!(cfg.runtime_offer_parallelism_max_workers, 2);
        assert_eq!(cfg.runtime_reservation_ttl_seconds, 900);
    }

    #[test]
    fn parse_program_config_multiple_keys_in_registry() {
        let mut raw = base_program_raw();
        raw["keys"]["registry"]
            .as_array_mut()
            .unwrap()
            .push(json!({"key_id": "key-main-2", "fingerprint": 987654321, "network": "mainnet"}));
        let cfg = parse_program_config(&raw).expect("config");
        assert_eq!(cfg.signer_key_registry.len(), 2);
        assert_eq!(cfg.signer_key_registry["key-main-2"].fingerprint, 987654321);
    }

    #[test]
    fn parse_program_config_empty_registry() {
        let mut raw = base_program_raw();
        raw["keys"]["registry"] = json!([]);
        let cfg = parse_program_config(&raw).expect("config");
        assert!(cfg.signer_key_registry.is_empty());
    }

    #[test]
    fn parse_program_config_missing_app() {
        let mut raw = base_program_raw();
        raw.as_object_mut().unwrap().remove("app");
        let err = parse_err(&raw);
        assert!(err.contains("Missing required field: app"));
    }

    #[test]
    fn parse_program_config_missing_runtime() {
        let mut raw = base_program_raw();
        raw.as_object_mut().unwrap().remove("runtime");
        let err = parse_err(&raw);
        assert!(err.contains("Missing required field: runtime"));
    }

    #[test]
    fn parse_program_config_missing_pushover_provider() {
        let mut raw = base_program_raw();
        raw["notifications"]["providers"] = json!([{"type": "slack"}]);
        let err = parse_err(&raw);
        assert!(err.contains("Missing notifications.providers entry with type=pushover"));
    }

    #[test]
    fn parse_program_config_invalid_venue_provider() {
        let mut raw = base_program_raw();
        raw["venues"]["offer_publish"]["provider"] = json!("binance");
        let err = parse_err(&raw);
        assert!(err.contains("venues.offer_publish.provider must be one of"));
    }

    #[test]
    fn parse_program_config_negative_minimum_fee_mojos() {
        let mut raw = base_program_raw();
        raw["coin_ops"]["minimum_fee_mojos"] = json!(-1);
        let err = parse_err(&raw);
        assert!(err.contains("coin_ops.minimum_fee_mojos must be >= 0"));
    }

    #[test]
    fn parse_program_config_invalid_trigger_mode() {
        let mut raw = base_program_raw();
        raw["chain_signals"]["tx_block_trigger"]["mode"] = json!("poll");
        let err = parse_err(&raw);
        assert!(err.contains("mode must be websocket"));
    }

    #[test]
    fn parse_program_config_reconnect_interval_too_low() {
        let mut raw = base_program_raw();
        raw["chain_signals"]["tx_block_trigger"]["websocket_reconnect_interval_seconds"] = json!(0);
        let err = parse_err(&raw);
        assert!(err.contains("websocket_reconnect_interval_seconds must be >= 1"));
    }

    #[test]
    fn parse_program_config_fallback_poll_interval_negative() {
        let mut raw = base_program_raw();
        raw["chain_signals"]["tx_block_trigger"]["fallback_poll_interval_seconds"] = json!(-5);
        let err = parse_err(&raw);
        assert!(err.contains("fallback_poll_interval_seconds must be >= 0"));
    }

    #[test]
    fn parse_program_config_registry_not_a_list() {
        let mut raw = base_program_raw();
        raw["keys"]["registry"] = json!("not-a-list");
        let err = parse_err(&raw);
        assert!(err.contains("keys.registry must be a list"));
    }

    #[test]
    fn parse_program_config_registry_entry_not_a_dict() {
        let mut raw = base_program_raw();
        raw["keys"]["registry"] = json!(["not-a-dict"]);
        let err = parse_err(&raw);
        assert!(err.contains("keys.registry entries must be mappings"));
    }

    #[test]
    fn parse_program_config_registry_empty_key_id() {
        let mut raw = base_program_raw();
        raw["keys"]["registry"] = json!([{"key_id": "", "fingerprint": 100}]);
        let err = parse_err(&raw);
        assert!(err.contains("key_id must be non-empty"));
    }

    #[test]
    fn parse_program_config_registry_invalid_fingerprint() {
        let mut raw = base_program_raw();
        raw["keys"]["registry"] = json!([{"key_id": "k1", "fingerprint": "abc"}]);
        let err = parse_err(&raw);
        assert!(err.contains("invalid fingerprint"));
    }

    #[test]
    fn parse_program_config_registry_non_positive_fingerprint() {
        let mut raw = base_program_raw();
        raw["keys"]["registry"] = json!([{"key_id": "k1", "fingerprint": 0}]);
        let err = parse_err(&raw);
        assert!(err.contains("fingerprint for key_id=k1 must be positive"));
    }

    #[test]
    fn parse_program_config_registry_duplicate_key_id() {
        let mut raw = base_program_raw();
        raw["keys"]["registry"] = json!([
            {"key_id": "k1", "fingerprint": 100},
            {"key_id": "k1", "fingerprint": 200}
        ]);
        let err = parse_err(&raw);
        assert!(err.contains("duplicate key_id"));
    }

    #[test]
    fn parse_program_config_registry_none_treated_as_empty() {
        let mut raw = base_program_raw();
        raw["keys"]["registry"] = json!(null);
        let cfg = parse_program_config(&raw).expect("config");
        assert!(cfg.signer_key_registry.is_empty());
    }

    #[test]
    fn resolves_testnet_dexie_default() {
        let url =
            resolve_dexie_base_url("testnet11", None, "https://api.dexie.space").expect("url");
        assert_eq!(url, "https://api-testnet.dexie.space");
    }

    #[test]
    fn maps_xch_to_txch_on_testnet() {
        assert_eq!(resolve_quote_asset_for_offer("xch", "testnet11"), "txch");
        assert_eq!(resolve_quote_asset_for_offer("xch", "mainnet"), "xch");
    }

    #[test]
    fn resolve_splash_base_url_defaults_to_program_base() {
        let splash = resolve_splash_base_url(None, "http://localhost:4000");
        assert_eq!(splash, "http://localhost:4000");
    }
}
