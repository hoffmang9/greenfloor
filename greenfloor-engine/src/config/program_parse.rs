//! Section parsers for `program.yaml` (extracted from `program.rs`).

use serde_json::Value;

use super::keys_registry::parse_signer_key_registry;
use super::program::{
    is_testnet_network, ManagerProgramConfig, DEFAULT_DEXIE_API_BASE, DEFAULT_SPLASH_API_BASE,
};
use super::yaml_fields::{
    config_err, optional_bool, parse_i64_field, parse_u64_field, req_mapping, req_mapping_from_map,
    req_str, req_value,
};
use crate::error::SignerResult;
use crate::paths::expand_home;

struct ParsedVenueConfig {
    dexie_api_base: String,
    splash_api_base: String,
    offer_publish_venue: String,
}

struct ParsedCoinOpsConfig {
    minimum_fee_mojos: u64,
    max_operations_per_run: i64,
    max_daily_fee_budget_mojos: i64,
    split_fee_mojos: i64,
    combine_fee_mojos: i64,
}

struct ParsedRuntimeConfig {
    loop_interval_seconds: u64,
    dry_run: bool,
    market_slot_count: u64,
    offer_parallelism_enabled: bool,
    offer_parallelism_max_workers: usize,
    reservation_ttl_seconds: u64,
    offer_bootstrap_wait_timeout_seconds: u64,
}

struct ParsedTxBlockConfig {
    mode: String,
    websocket_url: String,
    websocket_reconnect_interval_seconds: u64,
    fallback_poll_interval_seconds: u64,
}

struct SignerVaultIds {
    kms_key_id: String,
    kms_region: String,
    launcher_id: String,
}

/// Parse program config.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn parse_program_config(raw: &Value) -> SignerResult<ManagerProgramConfig> {
    reject_cloud_wallet(raw)?;

    let app = req_mapping(raw, "app")?;
    let runtime = req_mapping(raw, "runtime")?;
    let chain_signals = req_mapping(raw, "chain_signals")?;
    let dev = req_mapping(raw, "dev")?;
    let dev_python_min_version = parse_dev_python_min_version(dev)?;
    require_pushover_provider(raw)?;

    let tx_trigger = req_mapping_from_map(chain_signals, "tx_block_trigger")?;
    let coin_ops = raw.get("coin_ops").and_then(Value::as_object);
    let ParsedVenueConfig {
        dexie_api_base,
        splash_api_base,
        offer_publish_venue,
    } = parse_venue_config(raw)?;
    let ParsedCoinOpsConfig {
        minimum_fee_mojos: coin_ops_minimum_fee_mojos,
        max_operations_per_run: coin_ops_max_operations_per_run,
        max_daily_fee_budget_mojos: coin_ops_max_daily_fee_budget_mojos,
        split_fee_mojos: coin_ops_split_fee_mojos,
        combine_fee_mojos: coin_ops_combine_fee_mojos,
    } = parse_coin_ops_config(coin_ops)?;

    let app_log_level_was_missing = !app.contains_key("log_level");
    let app_log_level = normalize_manager_log_level(
        app.get("log_level")
            .and_then(Value::as_str)
            .unwrap_or("INFO"),
    );
    let network = req_str(app, "network")?;
    let home_dir = expand_home(req_str(app, "home_dir")?.trim());

    let ParsedRuntimeConfig {
        loop_interval_seconds: runtime_loop_interval_seconds,
        dry_run: runtime_dry_run,
        market_slot_count: runtime_market_slot_count,
        offer_parallelism_enabled: runtime_offer_parallelism_enabled,
        offer_parallelism_max_workers: runtime_offer_parallelism_max_workers,
        reservation_ttl_seconds: runtime_reservation_ttl_seconds,
        offer_bootstrap_wait_timeout_seconds: runtime_offer_bootstrap_wait_timeout_seconds,
    } = parse_runtime_config(runtime)?;

    let ParsedTxBlockConfig {
        mode: tx_block_trigger_mode,
        websocket_url: tx_block_websocket_url,
        websocket_reconnect_interval_seconds: tx_block_websocket_reconnect_interval_seconds,
        fallback_poll_interval_seconds: tx_block_fallback_poll_interval_seconds,
    } = parse_tx_block_config(tx_trigger, &network)?;

    let signer_key_registry = parse_signer_key_registry(raw)?;
    let SignerVaultIds {
        kms_key_id: signer_kms_key_id,
        kms_region: signer_kms_region,
        launcher_id: vault_launcher_id,
    } = parse_signer_vault_ids(raw);

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
        signer_kms_key_id,
        signer_kms_region,
        vault_launcher_id,
        dev_python_min_version,
        signer_key_registry,
    })
}

fn parse_venue_config(raw: &Value) -> SignerResult<ParsedVenueConfig> {
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
    if offer_publish_venue != "dexie" && offer_publish_venue != "splash" {
        return Err(config_err(
            "venues.offer_publish.provider must be one of: dexie, splash",
        ));
    }
    Ok(ParsedVenueConfig {
        dexie_api_base,
        splash_api_base,
        offer_publish_venue,
    })
}

fn parse_coin_ops_config(
    coin_ops: Option<&serde_json::Map<String, Value>>,
) -> SignerResult<ParsedCoinOpsConfig> {
    let raw_fee = parse_i64_field(
        coin_ops
            .and_then(|section| section.get("minimum_fee_mojos"))
            .unwrap_or(&Value::Number(10_000_000.into())),
        "coin_ops.minimum_fee_mojos",
    )?;
    if raw_fee < 0 {
        return Err(config_err("coin_ops.minimum_fee_mojos must be >= 0"));
    }
    let minimum_fee_mojos = u64::try_from(raw_fee)
        .map_err(|_| config_err("coin_ops.minimum_fee_mojos must fit in u64"))?;
    Ok(ParsedCoinOpsConfig {
        minimum_fee_mojos,
        max_operations_per_run: parse_i64_field(
            coin_ops
                .and_then(|section| section.get("max_operations_per_run"))
                .unwrap_or(&Value::Number(20.into())),
            "coin_ops.max_operations_per_run",
        )?,
        max_daily_fee_budget_mojos: parse_i64_field(
            coin_ops
                .and_then(|section| section.get("max_daily_fee_budget_mojos"))
                .unwrap_or(&Value::Number(0.into())),
            "coin_ops.max_daily_fee_budget_mojos",
        )?,
        split_fee_mojos: parse_i64_field(
            coin_ops
                .and_then(|section| section.get("split_fee_mojos"))
                .unwrap_or(&Value::Number(0.into())),
            "coin_ops.split_fee_mojos",
        )?,
        combine_fee_mojos: parse_i64_field(
            coin_ops
                .and_then(|section| section.get("combine_fee_mojos"))
                .unwrap_or(&Value::Number(0.into())),
            "coin_ops.combine_fee_mojos",
        )?,
    })
}

fn parse_runtime_config(
    runtime: &serde_json::Map<String, Value>,
) -> SignerResult<ParsedRuntimeConfig> {
    Ok(ParsedRuntimeConfig {
        loop_interval_seconds: parse_u64_field(
            req_value(runtime, "loop_interval_seconds")?,
            "runtime.loop_interval_seconds",
        )?,
        dry_run: optional_bool(runtime, "dry_run", false),
        market_slot_count: parse_u64_field(
            runtime
                .get("market_slot_count")
                .unwrap_or(&Value::Number(0.into())),
            "runtime.market_slot_count",
        )?,
        offer_parallelism_enabled: optional_bool(runtime, "offer_parallelism_enabled", false),
        offer_parallelism_max_workers: parse_u64_field(
            runtime
                .get("offer_parallelism_max_workers")
                .unwrap_or(&Value::Number(4.into())),
            "runtime.offer_parallelism_max_workers",
        )?
        .max(1)
        .try_into()
        .map_err(|_| config_err("runtime.offer_parallelism_max_workers must fit in usize"))?,
        reservation_ttl_seconds: parse_u64_field(
            runtime
                .get("reservation_ttl_seconds")
                .unwrap_or(&Value::Number(300.into())),
            "runtime.reservation_ttl_seconds",
        )?
        .max(30),
        offer_bootstrap_wait_timeout_seconds: runtime_timeout_seconds(
            runtime,
            "offer_bootstrap_wait_timeout_seconds",
            "cloud_wallet_bootstrap_wait_timeout_seconds",
            120,
            10,
        )?,
    })
}

fn parse_tx_block_config(
    tx_trigger: &serde_json::Map<String, Value>,
    network: &str,
) -> SignerResult<ParsedTxBlockConfig> {
    let mode = tx_trigger
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("websocket")
        .trim()
        .to_ascii_lowercase();
    if mode != "websocket" {
        return Err(config_err(
            "chain_signals.tx_block_trigger.mode must be websocket",
        ));
    }
    let mut websocket_url = tx_trigger
        .get("websocket_url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if websocket_url.is_empty() {
        websocket_url = if is_testnet_network(network) {
            "wss://testnet11.api.coinset.org/ws".to_string()
        } else {
            "wss://api.coinset.org/ws".to_string()
        };
    }
    let websocket_reconnect_interval_seconds = parse_u64_field(
        tx_trigger
            .get("websocket_reconnect_interval_seconds")
            .unwrap_or(&Value::Number(30.into())),
        "chain_signals.tx_block_trigger.websocket_reconnect_interval_seconds",
    )?;
    if websocket_reconnect_interval_seconds < 1 {
        return Err(config_err(
            "chain_signals.tx_block_trigger.websocket_reconnect_interval_seconds must be >= 1",
        ));
    }
    Ok(ParsedTxBlockConfig {
        mode,
        websocket_url,
        websocket_reconnect_interval_seconds,
        fallback_poll_interval_seconds: parse_u64_field(
            tx_trigger
                .get("fallback_poll_interval_seconds")
                .unwrap_or(&Value::Number(60.into())),
            "chain_signals.tx_block_trigger.fallback_poll_interval_seconds",
        )?,
    })
}

fn parse_dev_python_min_version(dev: &serde_json::Map<String, Value>) -> SignerResult<String> {
    let python = req_mapping_from_map(dev, "python")?;
    match python.get("min_version") {
        None => Ok("3.11".to_string()),
        Some(value) => {
            let text = value
                .as_str()
                .ok_or_else(|| config_err("dev.python.min_version must be a string"))?;
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Err(config_err(
                    "dev.python.min_version must be non-empty when set",
                ));
            }
            Ok(trimmed.to_string())
        }
    }
}

fn parse_signer_vault_ids(raw: &Value) -> SignerVaultIds {
    let signer = raw.get("signer").and_then(Value::as_object);
    let vault = raw.get("vault").and_then(Value::as_object);
    SignerVaultIds {
        kms_key_id: signer
            .and_then(|section| section.get("kms_key_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_string(),
        kms_region: signer
            .and_then(|section| section.get("kms_region"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("us-west-2")
            .to_string(),
        launcher_id: vault
            .and_then(|section| section.get("launcher_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_string(),
    }
}

fn reject_cloud_wallet(raw: &Value) -> SignerResult<()> {
    match raw.get("cloud_wallet") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::Object(map)) if map.is_empty() => Ok(()),
        Some(_) => Err(config_err(
            "cloud_wallet config is removed; use signer: and vault: blocks instead \
             (see config/program.yaml)",
        )),
    }
}

fn require_pushover_provider(raw: &Value) -> SignerResult<()> {
    let notifications = req_mapping(raw, "notifications")?;
    req_value(notifications, "low_inventory_alerts")?;
    let providers = req_value(notifications, "providers")?
        .as_array()
        .ok_or_else(|| config_err("notifications.providers must be a list"))?;
    if providers.iter().any(|provider| {
        provider
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|value| value.trim() == "pushover")
    }) {
        return Ok(());
    }
    Err(config_err(
        "Missing notifications.providers entry with type=pushover",
    ))
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

fn normalize_manager_log_level(log_level: &str) -> String {
    match log_level.trim().to_ascii_uppercase().as_str() {
        "CRITICAL" | "ERROR" | "WARNING" | "INFO" | "DEBUG" | "NOTSET" => {
            log_level.trim().to_ascii_uppercase()
        }
        _ => "INFO".to_string(),
    }
}
