use serde_json::Value;

use super::super::program::is_testnet_network;
use super::super::yaml_fields::{config_err, parse_u64_field};
use crate::error::SignerResult;

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
