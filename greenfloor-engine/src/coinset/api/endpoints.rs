use chia_protocol::SpendBundle;
use chia_traits::Streamable;
use serde_json::{json, Value};

use super::super::broadcast::broadcast_spend_bundle;
use super::super::msp::coinset_base_url_for_signer;
use super::rpc::{direct_coinset_client, post_coinset_rpc};
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};

/// Get fee estimate.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn get_fee_estimate(
    network: &str,
    base_url: Option<&str>,
    target_times: Vec<u64>,
    cost: u64,
    spend_count: Option<u64>,
) -> SignerResult<Value> {
    let mut body = json!({
        "target_times": target_times,
        "cost": cost.max(1),
    });
    if let Some(count) = spend_count.filter(|value| *value > 0) {
        body["spend_count"] = json!(count);
    }
    post_coinset_rpc(network, base_url, "get_fee_estimate", body).await
}

pub fn conservative_fee_from_payload(payload: &Value) -> Option<u64> {
    if !payload
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    if let Some(estimates) = payload.get("estimates").and_then(Value::as_array) {
        let mut valid = Vec::new();
        for value in estimates {
            if let Some(parsed) = value.as_u64() {
                valid.push(parsed);
            } else if let Some(parsed) = value.as_i64().filter(|v| *v >= 0) {
                if let Ok(parsed_u64) = u64::try_from(parsed) {
                    valid.push(parsed_u64);
                }
            }
        }
        if !valid.is_empty() {
            return Some(*valid.iter().max()?);
        }
    }
    payload.get("fee_estimate").and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_i64().and_then(|v| u64::try_from(v).ok()))
    })
}

/// Get conservative fee estimate.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn get_conservative_fee_estimate(
    network: &str,
    base_url: Option<&str>,
    cost: u64,
    spend_count: Option<u64>,
) -> SignerResult<Option<u64>> {
    let payload =
        get_fee_estimate(network, base_url, vec![300, 600, 1200], cost, spend_count).await?;
    Ok(conservative_fee_from_payload(&payload))
}

/// Conservative fee estimate using the signer's Coinset endpoint when configured.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn get_conservative_fee_estimate_for_signer(
    signer: &SignerConfig,
    cost: u64,
    spend_count: Option<u64>,
) -> SignerResult<Option<u64>> {
    get_conservative_fee_estimate(
        &signer.network,
        coinset_base_url_for_signer(signer),
        cost,
        spend_count,
    )
    .await
}

/// Get all mempool tx ids.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn get_all_mempool_tx_ids(
    network: &str,
    base_url: Option<&str>,
) -> SignerResult<Vec<String>> {
    let payload = post_coinset_rpc(network, base_url, "get_all_mempool_tx_ids", json!({})).await?;
    if !payload
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(Vec::new());
    }
    let tx_ids = payload
        .get("tx_ids")
        .or_else(|| payload.get("mempool_tx_ids"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(tx_ids
        .into_iter()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect())
}

/// Push tx hex.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn push_tx_hex(
    network: &str,
    base_url: Option<&str>,
    spend_bundle_hex: &str,
) -> SignerResult<Value> {
    let client = direct_coinset_client(network, base_url)?;
    let raw = spend_bundle_hex.trim().trim_start_matches("0x");
    let bytes =
        hex::decode(raw).map_err(|err| SignerError::Other(format!("invalid hex: {err}")))?;
    let spend_bundle = SpendBundle::from_bytes(&bytes)
        .map_err(|err: chia_traits::Error| SignerError::Other(err.to_string()))?;
    let result = broadcast_spend_bundle(&client, spend_bundle).await?;
    Ok(json!({
        "success": true,
        "status": result.status,
        "operation_id": result.operation_id,
    }))
}
