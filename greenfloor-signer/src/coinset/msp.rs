use chia_protocol::Bytes32;
use chia_sdk_coinset::{ChiaRpcClient, CoinRecord, CoinsetClient};
use serde::Deserialize;

use crate::coinset::is_xch_like_asset;
use crate::error::{SignerError, SignerResult};

pub const DEFAULT_MSP_BASE_URL: &str = "https://api-msp.coinset.org";

#[derive(Debug, Clone)]
pub struct MspCoinset {
    client: CoinsetClient,
}

impl MspCoinset {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            client: CoinsetClient::new(base_url.into().trim_end_matches('/').to_string()),
        }
    }

    pub fn for_network(network: &str, base_url: Option<&str>) -> SignerResult<Self> {
        let url = base_url
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_MSP_BASE_URL)
            .trim_end_matches('/')
            .to_string();
        match network {
            "mainnet" | "testnet11" => Ok(Self::new(url)),
            other => Err(SignerError::Other(format!("unsupported network: {other}"))),
        }
    }

    pub fn client(&self) -> &CoinsetClient {
        &self.client
    }

    pub async fn get_singleton_info(&self, launcher_id: Bytes32) -> SignerResult<SingletonInfo> {
        let response: GetSingletonInfoResponse = self
            .client
            .make_post_request(
                "get_singleton_info",
                serde_json::json!({
                    "launcher_id": format!("0x{}", hex::encode(launcher_id)),
                }),
            )
            .await
            .map_err(SignerError::from)?;
        if !response.success {
            return Err(SignerError::VaultSingletonNotFound);
        }
        Ok(SingletonInfo {
            launcher_id: response.launcher_id,
            singleton_type: response.singleton_type,
            coin_record: response.coin_record,
        })
    }

    pub async fn lookup_asset_by_symbol(&self, symbol: &str) -> SignerResult<Option<AssetInfo>> {
        let symbol = symbol.trim();
        if symbol.is_empty() {
            return Ok(None);
        }
        let response = self
            .client
            .make_post_request::<LookupAssetResponse, _>(
                "lookup_asset_by_symbol",
                serde_json::json!({ "symbol": symbol }),
            )
            .await;
        match response {
            Ok(body) if body.success => Ok(body.asset),
            Ok(_) => Ok(None),
            Err(_) => Ok(None),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SingletonInfo {
    pub launcher_id: String,
    pub singleton_type: Option<String>,
    pub coin_record: Option<CoinRecord>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssetInfo {
    pub asset_id: String,
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GetSingletonInfoResponse {
    success: bool,
    launcher_id: String,
    singleton_type: Option<String>,
    coin_record: Option<CoinRecord>,
}

#[derive(Debug, Deserialize)]
struct LookupAssetResponse {
    success: bool,
    asset: Option<AssetInfo>,
}

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

pub async fn resolve_offer_asset_ids(
    msp: &MspCoinset,
    base_asset: &str,
    quote_asset: &str,
) -> SignerResult<(String, String)> {
    let resolved_base = resolve_one_asset(msp, base_asset).await?;
    let resolved_quote = resolve_one_asset(msp, quote_asset).await?;
    if resolved_base == resolved_quote
        && !is_xch_like_asset(&resolved_base)
        && !is_xch_like_asset(&resolved_quote)
    {
        return Err(SignerError::ResolvedAssetsCollideForNonXchPair);
    }
    Ok((resolved_base, resolved_quote))
}

async fn resolve_one_asset(msp: &MspCoinset, raw: &str) -> SignerResult<String> {
    if let Ok(normalized) = normalize_asset_id(raw) {
        return Ok(normalized);
    }
    if let Ok(Some(asset)) = msp.lookup_asset_by_symbol(raw).await {
        return normalize_asset_id(&asset.asset_id);
    }
    Err(SignerError::Other(format!("asset_resolution_failed:{raw}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::members::hex_to_bytes32;

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
    async fn get_singleton_info_mock_shape() {
        let mut server = mockito::Server::new_async().await;
        let launcher = "aa".repeat(32);
        let _mock = server
            .mock("POST", "/get_singleton_info")
            .with_status(200)
            .with_body(format!(
                r#"{{"success":true,"launcher_id":"{launcher}","singleton_type":"standard","coin_record":null}}"#
            ))
            .create_async()
            .await;
        let msp = MspCoinset::new(server.url());
        let info = msp
            .get_singleton_info(hex_to_bytes32(&launcher).unwrap())
            .await
            .expect("singleton info");
        assert_eq!(info.launcher_id, launcher);
        assert_eq!(info.singleton_type.as_deref(), Some("standard"));
    }
}
