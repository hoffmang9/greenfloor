use chia_protocol::SpendBundle;
use chia_sdk_coinset::{ChiaRpcClient, CoinsetClient};
use chia_traits::Streamable;
use serde_json::{json, Value};

use crate::coinset::{
    broadcast_spend_bundle, client_for_network, coin_records_from_payload, direct_api,
    record_from_payload, resolve_direct_client, MspCoinset,
};
use crate::error::{SignerError, SignerResult};

pub fn direct_coinset_client(network: &str, base_url: Option<&str>) -> SignerResult<CoinsetClient> {
    let resolved = resolve_direct_client(network, base_url);
    Ok(CoinsetClient::new(resolved.base_url))
}

fn msp_coinset_client(network: &str, base_url: Option<&str>) -> SignerResult<CoinsetClient> {
    if let Some(url) = base_url.map(str::trim).filter(|value| !value.is_empty()) {
        Ok(MspCoinset::for_network(network, Some(url))?
            .client()
            .clone())
    } else {
        client_for_network(network)
    }
}

fn apply_testnet11_network(body: &mut Value, network: &str) {
    if direct_api::normalize_coinset_network(network) == "testnet11" {
        if let Some(obj) = body.as_object_mut() {
            obj.entry("network".to_string())
                .or_insert(json!("testnet11"));
        }
    }
}

async fn post_coinset_rpc_with(
    network: &str,
    base_url: Option<&str>,
    endpoint: &str,
    mut body: Value,
    client_for: fn(&str, Option<&str>) -> SignerResult<CoinsetClient>,
) -> SignerResult<Value> {
    let endpoint = endpoint.trim().trim_start_matches('/');
    if endpoint.is_empty() {
        return Err(SignerError::Other(
            "coinset endpoint is required".to_string(),
        ));
    }
    let network = direct_api::normalize_coinset_network(network);
    apply_testnet11_network(&mut body, network);
    let client = client_for(network, base_url)?;
    client
        .make_post_request(endpoint, body)
        .await
        .map_err(SignerError::from)
}

/// Script/scan Coinset RPC via the direct API host (`api.coinset.org` defaults).
pub async fn post_coinset_rpc(
    network: &str,
    base_url: Option<&str>,
    endpoint: &str,
    body: Value,
) -> SignerResult<Value> {
    post_coinset_rpc_with(network, base_url, endpoint, body, direct_coinset_client).await
}

async fn post_msp_coinset_rpc(
    network: &str,
    base_url: Option<&str>,
    endpoint: &str,
    body: Value,
) -> SignerResult<Value> {
    post_coinset_rpc_with(network, base_url, endpoint, body, msp_coinset_client).await
}

pub async fn post_coinset_coin_records(
    network: &str,
    base_url: Option<&str>,
    endpoint: &str,
    body: Value,
) -> SignerResult<Vec<Value>> {
    coin_records_from_payload(&post_coinset_rpc(network, base_url, endpoint, body).await?)
}

pub async fn post_coinset_record(
    network: &str,
    base_url: Option<&str>,
    endpoint: &str,
    body: Value,
    key: &str,
) -> SignerResult<Option<Value>> {
    let payload = post_coinset_rpc(network, base_url, endpoint, body).await?;
    Ok(record_from_payload(&payload, key)?.cloned())
}

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
    post_msp_coinset_rpc(network, base_url, "get_fee_estimate", body).await
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

pub async fn get_all_mempool_tx_ids(
    network: &str,
    base_url: Option<&str>,
) -> SignerResult<Vec<String>> {
    let payload =
        post_msp_coinset_rpc(network, base_url, "get_all_mempool_tx_ids", json!({})).await?;
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

    #[tokio::test]
    async fn get_all_mempool_tx_ids_via_msp_client() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_all_mempool_tx_ids")
            .with_status(200)
            .with_body(r#"{"success":true,"tx_ids":["0xabc"]}"#)
            .create_async()
            .await;

        let tx_ids = get_all_mempool_tx_ids("mainnet", Some(&server.url()))
            .await
            .expect("mempool tx ids");
        assert_eq!(tx_ids, vec!["0xabc".to_string()]);
    }

    #[tokio::test]
    async fn get_fee_estimate_via_msp_client() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_fee_estimate")
            .with_status(200)
            .with_body(r#"{"success":true,"estimates":[100,500]}"#)
            .create_async()
            .await;

        let payload = get_fee_estimate("mainnet", Some(&server.url()), vec![300], 1_000_000, None)
            .await
            .expect("fee estimate");
        assert_eq!(conservative_fee_from_payload(&payload), Some(500));
    }

    #[tokio::test]
    async fn post_coinset_rpc_get_all_mempool_tx_ids() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_all_mempool_tx_ids")
            .with_status(200)
            .with_body(r#"{"success":true,"mempool_tx_ids":["0xabc","0xdef"]}"#)
            .create_async()
            .await;

        let payload = post_coinset_rpc(
            "mainnet",
            Some(&server.url()),
            "get_all_mempool_tx_ids",
            json!({}),
        )
        .await
        .expect("mempool tx ids");
        assert_eq!(
            payload
                .get("mempool_tx_ids")
                .and_then(|value| value.as_array())
                .map(std::vec::Vec::len),
            Some(2)
        );
    }

    #[tokio::test]
    async fn post_coinset_rpc_coin_records_by_puzzle_hash_filters_via_parse() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(200)
            .with_body(r#"{"success":true,"coin_records":[{"coin":{"amount":1}},"bad"]}"#)
            .create_async()
            .await;

        let records = post_coinset_coin_records(
            "mainnet",
            Some(&server.url()),
            "get_coin_records_by_puzzle_hash",
            json!({"puzzle_hash": "0x11", "include_spent_coins": false}),
        )
        .await
        .expect("coin records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["coin"]["amount"], 1);
    }

    #[tokio::test]
    async fn post_coinset_rpc_get_coin_record_by_name() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_coin_record_by_name")
            .with_status(200)
            .with_body(r#"{"success":true,"coin_record":{"coin":{"amount":123}}}"#)
            .create_async()
            .await;

        let found = post_coinset_record(
            "mainnet",
            Some(&server.url()),
            "get_coin_record_by_name",
            json!({"name": "0x22"}),
            "coin_record",
        )
        .await
        .expect("coin record")
        .expect("some record");
        assert_eq!(found["coin"]["amount"], 123);
    }

    #[tokio::test]
    async fn post_coinset_rpc_get_blockchain_state() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_blockchain_state")
            .with_status(200)
            .with_body(r#"{"success":true,"blockchain_state":{"peak_height":1234}}"#)
            .create_async()
            .await;

        let state = post_coinset_record(
            "mainnet",
            Some(&server.url()),
            "get_blockchain_state",
            json!({}),
            "blockchain_state",
        )
        .await
        .expect("blockchain state")
        .expect("some state");
        assert_eq!(state["peak_height"], 1234);
    }

    #[tokio::test]
    async fn post_coinset_rpc_accepts_testnet_alias() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_blockchain_state")
            .with_status(200)
            .with_body(r#"{"success":true,"blockchain_state":{"peak_height":1}}"#)
            .create_async()
            .await;

        let state = post_coinset_record(
            "testnet",
            Some(&server.url()),
            "get_blockchain_state",
            json!({}),
            "blockchain_state",
        )
        .await
        .expect("testnet alias")
        .expect("some state");
        assert_eq!(state["peak_height"], 1);
    }

    #[tokio::test]
    async fn post_coinset_coin_records_fails_on_success_false() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_coin_records_by_puzzle_hash")
            .with_status(200)
            .with_body(r#"{"success":false,"error":"invalid puzzle hash"}"#)
            .create_async()
            .await;

        let err = post_coinset_coin_records(
            "mainnet",
            Some(&server.url()),
            "get_coin_records_by_puzzle_hash",
            json!({"puzzle_hash": "0x11", "include_spent_coins": false}),
        )
        .await
        .expect_err("success=false should fail");
        assert_eq!(err.to_string(), "coinset error: invalid puzzle hash");
    }

    #[tokio::test]
    async fn post_coinset_rpc_surfaces_http_503_as_coinset_error() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/get_blockchain_state")
            .with_status(503)
            .with_body("service unavailable")
            .create_async()
            .await;

        let err = post_coinset_rpc(
            "mainnet",
            Some(&server.url()),
            "get_blockchain_state",
            json!({}),
        )
        .await
        .expect_err("503 should fail");
        let message = err.to_string();
        assert!(message.starts_with("coinset error:"), "{message}");
        assert_eq!(
            message, "coinset error: error decoding response body",
            "unexpected coinset 503 error text"
        );
    }

    #[tokio::test]
    async fn push_tx_hex_returns_success_payload() {
        let bundle = SpendBundle::new(Vec::new(), chia_bls::Signature::default());
        let spend_bundle_hex = hex::encode(
            bundle
                .to_bytes()
                .expect("serialize empty spend bundle for push tx test"),
        );

        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/push_tx")
            .with_status(200)
            .with_body(r#"{"success":true,"status":"SUCCESS"}"#)
            .create_async()
            .await;

        let result = push_tx_hex("mainnet", Some(&server.url()), &spend_bundle_hex)
            .await
            .expect("push tx");
        assert_eq!(result.get("success").and_then(Value::as_bool), Some(true));
        assert_eq!(
            result.get("status").and_then(|value| value.as_str()),
            Some("SUCCESS")
        );
    }
}
