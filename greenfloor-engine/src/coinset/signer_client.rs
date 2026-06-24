//! Coinset HTTP client construction for signer-backed operator paths.

use chia_sdk_coinset::CoinsetClient;

use super::direct_api::{
    effective_coinset_base_url, normalize_coinset_network, MAINNET_DIRECT_BASE_URL,
};
use super::direct_coinset_client;
use crate::config::SignerConfig;
use crate::error::SignerResult;

/// Default Coinset HTTP host for signer-backed operator paths.
pub const DEFAULT_COINSET_BASE_URL: &str = MAINNET_DIRECT_BASE_URL;

/// Returns the configured Coinset base URL from signer config, if non-empty.
#[must_use]
pub fn coinset_base_url_for_signer(signer: &SignerConfig) -> Option<&str> {
    let url = signer.coinset_base_url.trim();
    if url.is_empty() {
        None
    } else {
        Some(url)
    }
}

/// Effective Coinset base URL for a signer on the given operator network.
#[must_use]
pub fn effective_coinset_base_url_for_signer_on_network(
    signer: &SignerConfig,
    network: &str,
) -> String {
    effective_coinset_base_url(network, &signer.coinset_base_url)
}

/// Coinset client for network.
///
/// # Errors
///
/// Returns an error if the network is unsupported.
pub fn client_for_network(network: &str) -> SignerResult<CoinsetClient> {
    direct_coinset_client(network, None)
}

/// Coinset client for signer config (network + effective `coinset_base_url`).
///
/// # Errors
///
/// Returns an error if the signer network is unsupported.
pub fn client_for_signer(signer: &SignerConfig) -> SignerResult<CoinsetClient> {
    let network = normalize_coinset_network(&signer.network);
    let base_url = effective_coinset_base_url(network, &signer.coinset_base_url);
    direct_coinset_client(network, Some(&base_url))
}

/// Coinset client for signer config.
///
/// # Errors
///
/// Returns an error if the signer network is unsupported.
pub fn client_for_config(config: &SignerConfig) -> SignerResult<CoinsetClient> {
    client_for_signer(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::signer_config::test_signer_config;

    #[test]
    fn coinset_base_url_for_signer_returns_none_when_unconfigured() {
        let signer = test_signer_config("");
        assert!(coinset_base_url_for_signer(&signer).is_none());
        let signer = test_signer_config("https://coinset.example.test");
        assert_eq!(
            coinset_base_url_for_signer(&signer),
            Some("https://coinset.example.test")
        );
    }
}
