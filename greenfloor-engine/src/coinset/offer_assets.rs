//! Coinset HTTP helpers for offer asset metadata lookup.

use chia_sdk_coinset::{ChiaRpcClient, CoinsetClient};
use serde::Deserialize;

use crate::error::SignerResult;

#[derive(Debug, Clone, Deserialize)]
pub struct AssetInfo {
    pub asset_id: String,
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LookupAssetResponse {
    success: bool,
    asset: Option<AssetInfo>,
}

/// Lookup asset metadata by ticker symbol via Coinset HTTP.
///
/// # Errors
///
/// Returns an error if the Coinset request fails.
pub async fn lookup_asset_by_symbol(
    client: &CoinsetClient,
    symbol: &str,
) -> SignerResult<Option<AssetInfo>> {
    let symbol = symbol.trim();
    if symbol.is_empty() {
        return Ok(None);
    }
    let response = client
        .make_post_request::<LookupAssetResponse, _>(
            "lookup_asset_by_symbol",
            serde_json::json!({ "symbol": symbol }),
        )
        .await;
    match response {
        Ok(body) if body.success => Ok(body.asset),
        Ok(_) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lookup_asset_by_symbol_mock_shape() {
        let asset_id = "aa".repeat(32);
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/lookup_asset_by_symbol")
            .with_status(200)
            .with_body(format!(
                r#"{{"success":true,"asset":{{"asset_id":"{asset_id}","symbol":"BYC"}}}}"#
            ))
            .create_async()
            .await;
        let client = CoinsetClient::new(server.url());
        let asset = lookup_asset_by_symbol(&client, "BYC")
            .await
            .expect("lookup");
        assert_eq!(
            asset.as_ref().and_then(|a| a.symbol.as_deref()),
            Some("BYC")
        );
    }

    #[tokio::test]
    async fn lookup_asset_by_symbol_propagates_transport_errors() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/lookup_asset_by_symbol")
            .with_status(500)
            .create_async()
            .await;
        let client = CoinsetClient::new(server.url());
        lookup_asset_by_symbol(&client, "BYC")
            .await
            .expect_err("transport error");
    }
}
