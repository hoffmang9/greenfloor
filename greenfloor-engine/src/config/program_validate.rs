use std::collections::HashSet;

use serde_json::Value;

use crate::error::{SignerError, SignerResult};

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

fn req_value<'a>(map: &'a serde_json::Map<String, Value>, key: &str) -> SignerResult<&'a Value> {
    map.get(key)
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
    Err(program_err(format!("invalid {context}")))
}

fn validate_keys_registry(raw: &Value) -> SignerResult<()> {
    let keys_root = raw.get("keys").and_then(Value::as_object);
    let registry_rows = keys_root
        .and_then(|keys| keys.get("registry"))
        .unwrap_or(&Value::Null);

    if registry_rows.is_null() {
        return Ok(());
    }

    let rows = registry_rows
        .as_array()
        .ok_or_else(|| program_err("keys.registry must be a list"))?;

    let mut seen_key_ids = HashSet::new();
    for row in rows {
        let row_map = row
            .as_object()
            .ok_or_else(|| program_err("keys.registry entries must be mappings"))?;
        let key_id = req_value(row_map, "key_id")?
            .as_str()
            .ok_or_else(|| program_err("keys.registry entry key_id must be non-empty"))?
            .trim()
            .to_string();
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
        if !seen_key_ids.insert(key_id.clone()) {
            return Err(program_err(format!(
                "duplicate key_id in keys.registry: {key_id}"
            )));
        }
    }
    Ok(())
}

fn validate_cloud_wallet(raw: &Value) -> SignerResult<()> {
    match raw.get("cloud_wallet") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::Object(map)) if map.is_empty() => Ok(()),
        Some(_) => Err(program_err(
            "cloud_wallet config is removed; use signer: and vault: blocks instead \
             (see config/program.yaml)",
        )),
    }
}

fn validate_offer_publish_provider(raw: &Value) -> SignerResult<()> {
    let venues = raw.get("venues").and_then(Value::as_object);
    let offer_publish = venues
        .and_then(|section| section.get("offer_publish"))
        .and_then(Value::as_object);
    let provider = offer_publish
        .and_then(|section| section.get("provider"))
        .and_then(Value::as_str)
        .unwrap_or("dexie")
        .trim()
        .to_ascii_lowercase();
    if provider != "dexie" && provider != "splash" {
        return Err(program_err(
            "venues.offer_publish.provider must be one of: dexie, splash",
        ));
    }
    Ok(())
}

fn validate_coin_ops_minimum_fee(raw: &Value) -> SignerResult<()> {
    let coin_ops = raw.get("coin_ops").and_then(Value::as_object);
    let minimum_fee_mojos = coin_ops
        .and_then(|section| section.get("minimum_fee_mojos"))
        .map(|raw| parse_i64_field(raw, "coin_ops.minimum_fee_mojos"))
        .transpose()
        .map_err(|_| program_err("coin_ops.minimum_fee_mojos must be an integer"))?
        .unwrap_or(10_000_000);
    if minimum_fee_mojos < 0 {
        return Err(program_err("coin_ops.minimum_fee_mojos must be >= 0"));
    }
    Ok(())
}

fn validate_tx_block_trigger(raw: &Value) -> SignerResult<()> {
    let chain_signals = req_mapping(raw, "chain_signals")?;
    let tx_trigger = req_value(chain_signals, "tx_block_trigger")?
        .as_object()
        .ok_or_else(|| program_err("chain_signals.tx_block_trigger must be a mapping"))?;

    let mode = tx_trigger
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("websocket")
        .trim()
        .to_ascii_lowercase();
    if mode != "websocket" {
        return Err(program_err(
            "chain_signals.tx_block_trigger.mode must be websocket",
        ));
    }

    let reconnect_interval = tx_trigger
        .get("websocket_reconnect_interval_seconds")
        .map(|raw| {
            parse_i64_field(
                raw,
                "chain_signals.tx_block_trigger.websocket_reconnect_interval_seconds",
            )
        })
        .transpose()
        .map_err(|_| {
            program_err(
                "chain_signals.tx_block_trigger.websocket_reconnect_interval_seconds must be an integer",
            )
        })?
        .unwrap_or(30);
    if reconnect_interval < 1 {
        return Err(program_err(
            "chain_signals.tx_block_trigger.websocket_reconnect_interval_seconds must be >= 1",
        ));
    }

    let fallback_poll = tx_trigger
        .get("fallback_poll_interval_seconds")
        .map(|raw| {
            parse_i64_field(
                raw,
                "chain_signals.tx_block_trigger.fallback_poll_interval_seconds",
            )
        })
        .transpose()
        .map_err(|_| {
            program_err(
                "chain_signals.tx_block_trigger.fallback_poll_interval_seconds must be an integer",
            )
        })?
        .unwrap_or(60);
    if fallback_poll < 0 {
        return Err(program_err(
            "chain_signals.tx_block_trigger.fallback_poll_interval_seconds must be >= 0",
        ));
    }
    Ok(())
}

fn validate_notifications_pushover(raw: &Value) -> SignerResult<()> {
    let notifications = req_mapping(raw, "notifications")?;
    req_value(notifications, "low_inventory_alerts")?;
    let providers = req_value(notifications, "providers")?
        .as_array()
        .ok_or_else(|| program_err("notifications.providers must be a list"))?;

    let pushover = providers.iter().find(|provider| {
        provider
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|value| value.trim() == "pushover")
    });
    if pushover.is_none() {
        return Err(program_err(
            "Missing notifications.providers entry with type=pushover",
        ));
    }
    Ok(())
}

pub fn validate_program_config(raw: &Value) -> SignerResult<()> {
    req_mapping(raw, "app")?;
    req_mapping(raw, "runtime")?;
    req_mapping(raw, "chain_signals")?;
    req_mapping(raw, "dev")?;
    validate_notifications_pushover(raw)?;
    validate_cloud_wallet(raw)?;
    validate_keys_registry(raw)?;
    validate_offer_publish_provider(raw)?;
    validate_coin_ops_minimum_fee(raw)?;
    validate_tx_block_trigger(raw)?;
    Ok(())
}
