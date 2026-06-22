use chia_sdk_coinset::{ChiaRpcClient, CoinsetClient};
use serde_json::Value;

use super::super::{
    direct_api,
    msp::{self, MspCoinset},
    parse::{coin_records_from_payload, record_from_payload},
};
use crate::error::{SignerError, SignerResult};

/// Direct coinset client.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn direct_coinset_client(network: &str, base_url: Option<&str>) -> SignerResult<CoinsetClient> {
    let resolved = direct_api::resolve_direct_client(network, base_url);
    Ok(CoinsetClient::new(resolved.base_url))
}

fn msp_coinset_client(network: &str, base_url: Option<&str>) -> SignerResult<CoinsetClient> {
    if let Some(url) = base_url.map(str::trim).filter(|value| !value.is_empty()) {
        Ok(MspCoinset::for_network(network, Some(url))?
            .client()
            .clone())
    } else {
        msp::client_for_network(network)
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
