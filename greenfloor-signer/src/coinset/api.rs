use chia_protocol::SpendBundle;
use chia_sdk_coinset::ChiaRpcClient;
use chia_traits::Streamable;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::coinset::{broadcast_spend_bundle, client_for_network, MspCoinset};
use crate::error::{SignerError, SignerResult};

#[derive(Debug, Deserialize)]
struct FeeEstimateResponse {
    success: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    estimates: Option<Vec<Value>>,
    #[serde(default)]
    fee_estimate: Option<Value>,
}

fn coinset_client(network: &str, base_url: Option<&str>) -> SignerResult<chia_sdk_coinset::CoinsetClient> {
    if let Some(url) = base_url.map(str::trim).filter(|value| !value.is_empty()) {
        Ok(MspCoinset::for_network(network, Some(url))?.client().clone())
    } else {
        client_for_network(network)
    }
}

pub async fn push_tx_hex(
    network: &str,
    base_url: Option<&str>,
    spend_bundle_hex: &str,
) -> SignerResult<Value> {
    let client = coinset_client(network, base_url)?;
    let raw = spend_bundle_hex.trim().trim_start_matches("0x");
    let bytes = hex::decode(raw).map_err(|err| SignerError::Other(format!("invalid hex: {err}")))?;
    let spend_bundle =
        SpendBundle::from_bytes(&bytes)
            .map_err(|err: chia_traits::Error| SignerError::Other(err.to_string()))?;
    let result = broadcast_spend_bundle(&client, spend_bundle).await?;
    Ok(json!({
        "success": true,
        "status": result.status,
        "operation_id": result.operation_id,
    }))
}

pub async fn get_fee_estimate(
    network: &str,
    base_url: Option<&str>,
    target_times: Vec<u64>,
    cost: u64,
    spend_count: Option<u64>,
) -> SignerResult<Value> {
    let msp = if let Some(url) = base_url.map(str::trim).filter(|value| !value.is_empty()) {
        MspCoinset::for_network(network, Some(url))?
    } else {
        MspCoinset::for_network(network, None)?
    };
    let mut body = json!({
        "target_times": target_times,
        "cost": cost.max(1),
    });
    if let Some(count) = spend_count.filter(|value| *value > 0) {
        body["spend_count"] = json!(count);
    }
    if network.trim().eq_ignore_ascii_case("testnet11") {
        if let Some(obj) = body.as_object_mut() {
            obj.entry("network".to_string()).or_insert(json!("testnet11"));
        }
    }
    let response: FeeEstimateResponse = msp
        .client()
        .make_post_request("get_fee_estimate", body)
        .await
        .map_err(SignerError::from)?;
    if !response.success {
        return Ok(json!({
            "success": false,
            "error": response.error.unwrap_or_else(|| "coinset_fee_estimate_unsuccessful".to_string()),
        }));
    }
    Ok(json!({
        "success": true,
        "estimates": response.estimates,
        "fee_estimate": response.fee_estimate,
    }))
}

pub fn conservative_fee_from_payload(payload: &Value) -> Option<u64> {
    if !payload.get("success").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    if let Some(estimates) = payload.get("estimates").and_then(Value::as_array) {
        let mut valid = Vec::new();
        for value in estimates {
            if let Some(parsed) = value.as_u64() {
                valid.push(parsed);
            } else if let Some(parsed) = value.as_i64().filter(|v| *v >= 0) {
                valid.push(parsed as u64);
            }
        }
        if !valid.is_empty() {
            return Some(*valid.iter().max()?);
        }
    }
    payload
        .get("fee_estimate")
        .and_then(|value| value.as_u64().or_else(|| value.as_i64().filter(|v| *v >= 0).map(|v| v as u64)))
}

pub async fn get_conservative_fee_estimate(
    network: &str,
    base_url: Option<&str>,
    cost: u64,
    spend_count: Option<u64>,
) -> SignerResult<Option<u64>> {
    let payload = get_fee_estimate(
        network,
        base_url,
        vec![300, 600, 1200],
        cost,
        spend_count,
    )
    .await?;
    Ok(conservative_fee_from_payload(&payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conservative_fee_uses_max_estimate() {
        let payload = json!({"success": true, "estimates": [100, 500, 200]});
        assert_eq!(conservative_fee_from_payload(&payload), Some(500));
    }

    #[test]
    fn conservative_fee_falls_back_to_fee_estimate_field() {
        let payload = json!({"success": true, "fee_estimate": 42});
        assert_eq!(conservative_fee_from_payload(&payload), Some(42));
    }

    #[test]
    fn conservative_fee_returns_none_on_failure() {
        let payload = json!({"success": false});
        assert_eq!(conservative_fee_from_payload(&payload), None);
    }
}
