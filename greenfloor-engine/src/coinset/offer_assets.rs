//! Offer asset resolution against the direct Coinset HTTP API.

use chia_sdk_coinset::{ChiaRpcClient, CoinsetClient};
use serde::Deserialize;

use super::asset::is_xch_like_asset;
use crate::error::{SignerError, SignerResult};

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

/// Lookup asset metadata by ticker symbol.
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
        _ => Ok(None),
    }
}

/// Normalize asset id.
///
/// # Errors
///
/// Returns an error if the asset id is invalid.
pub fn normalize_asset_id(raw: &str) -> SignerResult<String> {
    let trimmed = raw.trim().to_lowercase();
    if trimmed.is_empty() {
        return Err(SignerError::MissingAssetId);
    }
    if matches!(trimmed.as_str(), "xch" | "txch" | "1") {
        return Ok(trimmed);
    }
    if trimmed.len() == 64 && trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Ok(trimmed);
    }
    Err(SignerError::Other(format!(
        "invalid asset id (expected 64-hex cat id or xch/txch): {raw}"
    )))
}

/// Resolve offer asset ids.
///
/// # Errors
///
/// Returns an error if asset resolution fails.
pub async fn resolve_offer_asset_ids(
    client: &CoinsetClient,
    base_asset: &str,
    quote_asset: &str,
) -> SignerResult<(String, String)> {
    let resolved_base = resolve_one_asset(client, base_asset).await?;
    let resolved_quote = resolve_one_asset(client, quote_asset).await?;
    if resolved_base == resolved_quote
        && !is_xch_like_asset(&resolved_base)
        && !is_xch_like_asset(&resolved_quote)
    {
        return Err(SignerError::ResolvedAssetsCollideForNonXchPair);
    }
    Ok((resolved_base, resolved_quote))
}

async fn resolve_one_asset(client: &CoinsetClient, raw: &str) -> SignerResult<String> {
    if let Ok(normalized) = normalize_asset_id(raw) {
        return Ok(normalized);
    }
    if let Ok(Some(asset)) = lookup_asset_by_symbol(client, raw).await {
        return normalize_asset_id(&asset.asset_id);
    }
    Err(SignerError::Other(format!("asset_resolution_failed:{raw}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_asset_id_accepts_xch_and_hex() {
        assert_eq!(normalize_asset_id("xch").unwrap(), "xch");
        assert_eq!(normalize_asset_id("TXCH").unwrap(), "txch");
        let cat = "a".repeat(64);
        assert_eq!(normalize_asset_id(&cat).unwrap(), cat);
    }

    #[test]
    fn normalize_asset_id_rejects_invalid() {
        assert!(normalize_asset_id("").is_err());
        assert!(normalize_asset_id("Asset_foo").is_err());
    }

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
}
