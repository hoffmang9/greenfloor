//! Section parsers for `program.yaml` (`coin_ops`, runtime, storage, venue, guards, `tx_block`).

use serde_json::Value;

use super::super::program::{is_testnet_network, DEFAULT_DEXIE_API_BASE, DEFAULT_SPLASH_API_BASE};
use super::super::venue::Venue;
use super::super::yaml_fields::{
    config_err, optional_bool, optional_str_section, optional_trimmed_str_section, parse_i64_field,
    parse_u64_field, req_mapping, req_mapping_from_map, req_value,
};
use crate::error::SignerResult;
use crate::storage::DEFAULT_AUDIT_RETENTION_DAYS;

pub(super) fn reject_cloud_wallet(raw: &Value) -> SignerResult<()> {
    match raw.get("cloud_wallet") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::Object(map)) if map.is_empty() => Ok(()),
        Some(_) => Err(config_err(
            "cloud_wallet config is removed; use signer: and vault: blocks instead \
             (see config/program.yaml)",
        )),
    }
}

pub(super) fn require_pushover_provider(raw: &Value) -> SignerResult<()> {
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

pub(super) fn parse_dev_python_min_version(
    dev: &serde_json::Map<String, Value>,
) -> SignerResult<String> {
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

fn venues_subsection<'a>(raw: &'a Value, name: &str) -> Option<&'a serde_json::Map<String, Value>> {
    raw.get("venues")
        .and_then(Value::as_object)
        .and_then(|venues| venues.get(name))
        .and_then(Value::as_object)
}

fn normalize_api_base(raw: Option<&Value>, default: &str) -> String {
    raw.and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default)
        .trim_end_matches('/')
        .to_string()
}

fn coin_ops_i64_field(
    section: Option<&serde_json::Map<String, Value>>,
    key: &str,
    default: i64,
) -> SignerResult<i64> {
    parse_i64_field(
        section
            .and_then(|map| map.get(key))
            .unwrap_or(&Value::Number(default.into())),
        &format!("coin_ops.{key}"),
    )
}

#[allow(clippy::struct_field_names)]
pub(super) struct CoinOpsFields {
    pub coin_ops_minimum_fee_mojos: u64,
    pub coin_ops_max_operations_per_run: i64,
    pub coin_ops_max_daily_fee_budget_mojos: i64,
    pub coin_ops_split_fee_mojos: i64,
    pub coin_ops_combine_fee_mojos: i64,
}

pub(super) fn parse_coin_ops_config(
    coin_ops: Option<&serde_json::Map<String, Value>>,
) -> SignerResult<CoinOpsFields> {
    let raw_fee = coin_ops_i64_field(coin_ops, "minimum_fee_mojos", 10_000_000)?;
    if raw_fee < 0 {
        return Err(config_err("coin_ops.minimum_fee_mojos must be >= 0"));
    }
    let coin_ops_minimum_fee_mojos = u64::try_from(raw_fee)
        .map_err(|_| config_err("coin_ops.minimum_fee_mojos must fit in u64"))?;
    Ok(CoinOpsFields {
        coin_ops_minimum_fee_mojos,
        coin_ops_max_operations_per_run: coin_ops_i64_field(
            coin_ops,
            "max_operations_per_run",
            20,
        )?,
        coin_ops_max_daily_fee_budget_mojos: coin_ops_i64_field(
            coin_ops,
            "max_daily_fee_budget_mojos",
            0,
        )?,
        coin_ops_split_fee_mojos: coin_ops_i64_field(coin_ops, "split_fee_mojos", 0)?,
        coin_ops_combine_fee_mojos: coin_ops_i64_field(coin_ops, "combine_fee_mojos", 0)?,
    })
}

#[allow(clippy::struct_field_names)]
pub(super) struct RuntimeFields {
    pub runtime_loop_interval_seconds: u64,
    pub runtime_dry_run: bool,
    pub runtime_market_slot_count: u64,
    pub runtime_offer_parallelism_enabled: bool,
    pub runtime_offer_parallelism_max_workers: usize,
    pub runtime_reservation_ttl_seconds: u64,
    pub runtime_offer_bootstrap_wait_timeout_seconds: u64,
}

pub(super) fn parse_runtime_config(
    runtime: &serde_json::Map<String, Value>,
) -> SignerResult<RuntimeFields> {
    Ok(RuntimeFields {
        runtime_loop_interval_seconds: parse_u64_field(
            req_value(runtime, "loop_interval_seconds")?,
            "runtime.loop_interval_seconds",
        )?,
        runtime_dry_run: optional_bool(runtime, "dry_run", false),
        runtime_market_slot_count: parse_u64_field(
            runtime
                .get("market_slot_count")
                .unwrap_or(&Value::Number(0.into())),
            "runtime.market_slot_count",
        )?,
        runtime_offer_parallelism_enabled: optional_bool(
            runtime,
            "offer_parallelism_enabled",
            false,
        ),
        runtime_offer_parallelism_max_workers: parse_u64_field(
            runtime
                .get("offer_parallelism_max_workers")
                .unwrap_or(&Value::Number(4.into())),
            "runtime.offer_parallelism_max_workers",
        )?
        .max(1)
        .try_into()
        .map_err(|_| config_err("runtime.offer_parallelism_max_workers must fit in usize"))?,
        runtime_reservation_ttl_seconds: parse_u64_field(
            runtime
                .get("reservation_ttl_seconds")
                .unwrap_or(&Value::Number(300.into())),
            "runtime.reservation_ttl_seconds",
        )?
        .max(30),
        runtime_offer_bootstrap_wait_timeout_seconds: runtime_timeout_seconds(
            runtime,
            "offer_bootstrap_wait_timeout_seconds",
            "cloud_wallet_bootstrap_wait_timeout_seconds",
            120,
            10,
        )?,
    })
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

pub(super) struct StorageFields {
    pub storage_audit_retention_days: u64,
}

pub(super) fn parse_storage_config(
    storage: Option<&serde_json::Map<String, Value>>,
) -> SignerResult<StorageFields> {
    let section = storage.cloned().unwrap_or_default();
    Ok(StorageFields {
        storage_audit_retention_days: parse_u64_field(
            section
                .get("audit_retention_days")
                .unwrap_or(&Value::Number(DEFAULT_AUDIT_RETENTION_DAYS.into())),
            "storage.audit_retention_days",
        )?
        .max(1),
    })
}

pub(super) struct SignerVaultFields {
    pub signer_kms_key_id: String,
    pub signer_kms_region: String,
    pub vault_launcher_id: String,
}

pub(super) fn parse_signer_vault_ids(raw: &Value) -> SignerVaultFields {
    let signer = raw.get("signer").and_then(Value::as_object);
    let vault = raw.get("vault").and_then(Value::as_object);
    SignerVaultFields {
        signer_kms_key_id: optional_trimmed_str_section(signer, "kms_key_id"),
        signer_kms_region: optional_str_section(signer, "kms_region", "us-west-2"),
        vault_launcher_id: optional_trimmed_str_section(vault, "launcher_id"),
    }
}

pub(super) struct VenueFields {
    pub dexie_api_base: String,
    pub splash_api_base: String,
    pub offer_publish_venue: String,
}

pub(super) fn parse_venue_config(raw: &Value) -> SignerResult<VenueFields> {
    let raw_venue = optional_str_section(
        venues_subsection(raw, "offer_publish"),
        "provider",
        "coinset",
    );
    let offer_publish_venue = Venue::parse(&raw_venue)
        .map_err(|_| {
            config_err("venues.offer_publish.provider must be one of: coinset, dexie, splash")
        })?
        .as_str()
        .to_string();
    Ok(VenueFields {
        dexie_api_base: normalize_api_base(
            venues_subsection(raw, "dexie").and_then(|section| section.get("api_base")),
            DEFAULT_DEXIE_API_BASE,
        ),
        splash_api_base: normalize_api_base(
            venues_subsection(raw, "splash").and_then(|section| section.get("api_base")),
            DEFAULT_SPLASH_API_BASE,
        ),
        offer_publish_venue,
    })
}

#[allow(clippy::struct_field_names)]
pub(super) struct TxBlockFields {
    pub tx_block_trigger_mode: String,
    pub tx_block_websocket_url: String,
    pub tx_block_websocket_reconnect_interval_seconds: u64,
    pub tx_block_fallback_poll_interval_seconds: u64,
}

pub(super) fn parse_tx_block_config(
    tx_trigger: &serde_json::Map<String, Value>,
    network: &str,
) -> SignerResult<TxBlockFields> {
    let tx_block_trigger_mode = tx_trigger
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("websocket")
        .trim()
        .to_ascii_lowercase();
    if tx_block_trigger_mode != "websocket" {
        return Err(config_err(
            "chain_signals.tx_block_trigger.mode must be websocket",
        ));
    }
    let tx_block_websocket_url = tx_trigger
        .get("websocket_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map_or_else(
            || {
                if is_testnet_network(network) {
                    "wss://testnet11.api.coinset.org/ws".to_string()
                } else {
                    "wss://api.coinset.org/ws".to_string()
                }
            },
            str::to_string,
        );
    let tx_block_websocket_reconnect_interval_seconds = parse_u64_field(
        tx_trigger
            .get("websocket_reconnect_interval_seconds")
            .unwrap_or(&Value::Number(30.into())),
        "chain_signals.tx_block_trigger.websocket_reconnect_interval_seconds",
    )?;
    if tx_block_websocket_reconnect_interval_seconds < 1 {
        return Err(config_err(
            "chain_signals.tx_block_trigger.websocket_reconnect_interval_seconds must be >= 1",
        ));
    }
    Ok(TxBlockFields {
        tx_block_trigger_mode,
        tx_block_websocket_url,
        tx_block_websocket_reconnect_interval_seconds,
        tx_block_fallback_poll_interval_seconds: parse_u64_field(
            tx_trigger
                .get("fallback_poll_interval_seconds")
                .unwrap_or(&Value::Number(60.into())),
            "chain_signals.tx_block_trigger.fallback_poll_interval_seconds",
        )?,
    })
}
