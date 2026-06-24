use chia_protocol::Bytes32;
use chia_sdk_coinset::{ChiaRpcClient, CoinRecord, CoinsetClient};
use serde::Deserialize;

use super::asset::is_xch_like_asset;
use super::direct_api;
use crate::coinset::direct_coinset_client;
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};

/// Default Coinset HTTP host for signer-backed operator paths.
pub const DEFAULT_COINSET_BASE_URL: &str = direct_api::MAINNET_DIRECT_BASE_URL;

/// Backward-compatible alias; new code should use [`DEFAULT_COINSET_BASE_URL`].
pub const DEFAULT_MSP_BASE_URL: &str = DEFAULT_COINSET_BASE_URL;

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

    /// For network.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn for_network(network: &str, base_url: Option<&str>) -> SignerResult<Self> {
        let network = direct_api::normalize_coinset_network(network);
        match network {
            "mainnet" | "testnet11" => Ok(Self::new(resolve_coinset_base_url(network, base_url))),
            other => Err(SignerError::Other(format!("unsupported network: {other}"))),
        }
    }

    /// MSP client scoped to signer config (`network` + optional `coinset_msp_base_url`).
    ///
    /// # Errors
    ///
    /// Returns an error if the signer network is unsupported.
    pub fn for_signer(signer: &SignerConfig) -> SignerResult<Self> {
        Self::for_network(&signer.network, msp_base_url_for_signer(signer))
    }

    #[must_use]
    pub fn client(&self) -> &CoinsetClient {
        &self.client
    }

    /// Get singleton info.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
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

    /// Lookup asset by symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
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
            _ => Ok(None),
        }
    }
}

/// Client for network.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn client_for_network(network: &str) -> SignerResult<CoinsetClient> {
    direct_coinset_client(network, None)
}

/// Returns the configured Coinset base URL from signer config, if non-empty.
#[must_use]
pub fn coinset_base_url_for_signer(signer: &SignerConfig) -> Option<&str> {
    msp_base_url_for_signer(signer)
}

/// Resolve the Coinset base URL from signer config, if configured.
#[must_use]
pub fn msp_base_url_for_signer(signer: &SignerConfig) -> Option<&str> {
    let url = signer.coinset_msp_base_url.trim();
    if url.is_empty() {
        None
    } else {
        Some(url)
    }
}

fn resolve_coinset_base_url(network: &str, base_url: Option<&str>) -> String {
    direct_api::resolve_direct_coinset_base_url(network, base_url)
}

/// Coinset client for signer config (network + MSP base URL).
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn client_for_signer(signer: &SignerConfig) -> SignerResult<CoinsetClient> {
    direct_coinset_client(&signer.network, coinset_base_url_for_signer(signer))
}

/// Client for config.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn client_for_config(config: &SignerConfig) -> SignerResult<CoinsetClient> {
    client_for_signer(config)
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

/// Normalize asset id.
///
/// # Errors
///
/// Returns an error if the operation fails.
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
/// Returns an error if the operation fails.
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
    use crate::hex::hex_to_bytes32;

    #[test]
    fn msp_base_url_for_signer_returns_none_when_unconfigured() {
        use crate::test_support::signer_config::test_signer_config;

        let signer = test_signer_config("");
        assert!(msp_base_url_for_signer(&signer).is_none());
        let signer = test_signer_config("https://msp.example.test");
        assert_eq!(
            msp_base_url_for_signer(&signer),
            Some("https://msp.example.test")
        );
    }

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
