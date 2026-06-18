use serde_json::Value;

use super::keys_registry::parse_signer_key_registry;
use super::yaml_fields::{
    config_err, parse_i64_field, req_mapping, req_mapping_from_map, req_value,
};
use crate::error::SignerResult;

fn validate_cloud_wallet(raw: &Value) -> SignerResult<()> {
    match raw.get("cloud_wallet") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::Object(map)) if map.is_empty() => Ok(()),
        Some(_) => Err(config_err(
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
        return Err(config_err(
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
        .map_err(|_| config_err("coin_ops.minimum_fee_mojos must be an integer"))?
        .unwrap_or(10_000_000);
    if minimum_fee_mojos < 0 {
        return Err(config_err("coin_ops.minimum_fee_mojos must be >= 0"));
    }
    Ok(())
}

fn validate_tx_block_trigger(raw: &Value) -> SignerResult<()> {
    let chain_signals = req_mapping(raw, "chain_signals")?;
    let tx_trigger = req_mapping_from_map(chain_signals, "tx_block_trigger")?;

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
            config_err(
                "chain_signals.tx_block_trigger.websocket_reconnect_interval_seconds must be an integer",
            )
        })?
        .unwrap_or(30);
    if reconnect_interval < 1 {
        return Err(config_err(
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
            config_err(
                "chain_signals.tx_block_trigger.fallback_poll_interval_seconds must be an integer",
            )
        })?
        .unwrap_or(60);
    if fallback_poll < 0 {
        return Err(config_err(
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
        .ok_or_else(|| config_err("notifications.providers must be a list"))?;

    let pushover = providers.iter().find(|provider| {
        provider
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|value| value.trim() == "pushover")
    });
    if pushover.is_none() {
        return Err(config_err(
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
    parse_signer_key_registry(raw).map(|_| ())?;
    validate_offer_publish_provider(raw)?;
    validate_coin_ops_minimum_fee(raw)?;
    validate_tx_block_trigger(raw)?;
    Ok(())
}
