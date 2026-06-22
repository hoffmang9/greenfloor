use chia_sdk_coinset::{ChiaRpcClient, CoinsetClient};
use serde_json::Value;

use crate::coinset::{
    client_for_network, coin_records_from_payload, direct_api, record_from_payload,
    resolve_direct_client, MspCoinset,
};
use crate::error::{SignerError, SignerResult};

/// Direct coinset client.
///
/// # Errors
///
/// Returns an error if the operation fails.
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
                .or_insert(serde_json::json!("testnet11"));
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
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn post_coinset_rpc(
    network: &str,
    base_url: Option<&str>,
    endpoint: &str,
    body: Value,
) -> SignerResult<Value> {
    post_coinset_rpc_with(network, base_url, endpoint, body, direct_coinset_client).await
}

pub(super) async fn post_msp_coinset_rpc(
    network: &str,
    base_url: Option<&str>,
    endpoint: &str,
    body: Value,
) -> SignerResult<Value> {
    post_coinset_rpc_with(network, base_url, endpoint, body, msp_coinset_client).await
}

/// Post coinset coin records.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn post_coinset_coin_records(
    network: &str,
    base_url: Option<&str>,
    endpoint: &str,
    body: Value,
) -> SignerResult<Vec<Value>> {
    coin_records_from_payload(&post_coinset_rpc(network, base_url, endpoint, body).await?)
}

/// Post coinset record.
///
/// # Errors
///
/// Returns an error if the operation fails.
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
