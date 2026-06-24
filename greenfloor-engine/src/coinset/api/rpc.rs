use chia_sdk_coinset::{ChiaRpcClient, CoinsetClient};
use serde_json::{json, Value};

use super::super::{
    direct_api,
    msp::{self, MspCoinset},
    pagination::coin_records_from_json_endpoint,
    parse::{coin_records_from_payload, pagination_from_payload, record_from_payload},
    retry::with_script_retries,
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
                .or_insert(json!("testnet11"));
        }
    }
}

fn normalize_coinset_endpoint(endpoint: &str) -> SignerResult<&str> {
    let endpoint = endpoint.trim().trim_start_matches('/');
    if endpoint.is_empty() {
        return Err(SignerError::Other(
            "coinset endpoint is required".to_string(),
        ));
    }
    Ok(endpoint)
}

async fn post_coinset_rpc_with(
    network: &str,
    base_url: Option<&str>,
    endpoint: &str,
    mut body: Value,
    client_for: fn(&str, Option<&str>) -> SignerResult<CoinsetClient>,
) -> SignerResult<Value> {
    let endpoint = normalize_coinset_endpoint(endpoint)?;
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

/// Fetch all coin records from a JSON coin-record endpoint, following cursor pages.
///
/// # Errors
///
/// Returns an error if any page fails or a truncated page lacks `next_cursor`.
pub(crate) async fn fetch_paginated_coin_records_json(
    network: &str,
    base_url: Option<&str>,
    endpoint: &str,
    body: Value,
) -> SignerResult<Vec<Value>> {
    let endpoint_for_pages = normalize_coinset_endpoint(endpoint)?.to_string();
    let endpoint_for_closure = endpoint_for_pages.clone();
    let network = network.to_string();
    let base_url = base_url.map(str::to_string);
    coin_records_from_json_endpoint(&endpoint_for_pages, move |cursor| {
        let network = network.clone();
        let base_url = base_url.clone();
        let endpoint = endpoint_for_closure.clone();
        let mut page_body = body.clone();
        async move {
            if let Some(cursor) = cursor {
                page_body["cursor"] = json!(cursor);
            }
            let payload = with_script_retries(|| async {
                post_coinset_rpc(&network, base_url.as_deref(), &endpoint, page_body.clone()).await
            })
            .await?;
            let records = coin_records_from_payload(&payload)?;
            let pagination = pagination_from_payload(&payload);
            Ok((records, pagination))
        }
    })
    .await
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
    fetch_paginated_coin_records_json(network, base_url, endpoint, body).await
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
