use chia_protocol::SpendBundle;
use chia_sdk_coinset::ChiaRpcClient;
use chia_traits::Streamable;
use serde_json::{json, Value};

use crate::coinset::{broadcast_spend_bundle, client_for_network, MspCoinset};
use crate::error::{SignerError, SignerResult};

fn coinset_client(
    network: &str,
    base_url: Option<&str>,
) -> SignerResult<chia_sdk_coinset::CoinsetClient> {
    if let Some(url) = base_url.map(str::trim).filter(|value| !value.is_empty()) {
        Ok(MspCoinset::for_network(network, Some(url))?
            .client()
            .clone())
    } else {
        client_for_network(network)
    }
}

fn apply_testnet11_network(body: &mut Value, network: &str) {
    if network.trim().eq_ignore_ascii_case("testnet11") {
        if let Some(obj) = body.as_object_mut() {
            obj.entry("network".to_string())
                .or_insert(json!("testnet11"));
        }
    }
}

pub async fn post_coinset_rpc(
    network: &str,
    base_url: Option<&str>,
    endpoint: &str,
    mut body: Value,
) -> SignerResult<Value> {
    let endpoint = endpoint.trim().trim_start_matches('/');
    if endpoint.is_empty() {
        return Err(SignerError::Other(
            "coinset endpoint is required".to_string(),
        ));
    }
    apply_testnet11_network(&mut body, network);
    let msp = if let Some(url) = base_url.map(str::trim).filter(|value| !value.is_empty()) {
        MspCoinset::for_network(network, Some(url))?
    } else {
        MspCoinset::for_network(network, None)?
    };
    msp.client()
        .make_post_request(endpoint, body)
        .await
        .map_err(SignerError::from)
}

pub async fn push_tx_hex(
    network: &str,
    base_url: Option<&str>,
    spend_bundle_hex: &str,
) -> SignerResult<Value> {
    let client = coinset_client(network, base_url)?;
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
                valid.push(parsed as u64);
            }
        }
        if !valid.is_empty() {
            return Some(*valid.iter().max()?);
        }
    }
    payload.get("fee_estimate").and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_i64().filter(|v| *v >= 0).map(|v| v as u64))
    })
}

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
