//! Coinset HTTP client construction for signer-backed operator paths.
//!
//! Signer Coinset IO keys off the **operator network** argument (typically `program.network`
//! or a CLI `--network` override), not an stale `signer.network` field that may lag until
//! command-specific context is applied.

use chia_sdk_coinset::CoinsetClient;

use super::direct_api::resolve_coinset_endpoint;
use super::direct_coinset_client;
use crate::config::SignerConfig;
use crate::error::SignerResult;

/// Coinset client for signer config on the given operator network.
///
/// # Errors
///
/// Returns an error if the operator network is unsupported.
pub fn client_for_signer_on_network(
    signer: &SignerConfig,
    operator_network: &str,
) -> SignerResult<CoinsetClient> {
    let endpoint = resolve_coinset_endpoint(operator_network, &signer.coinset_base_url, None);
    direct_coinset_client(endpoint.network, Some(&endpoint.base_url))
}

/// Coinset client for signer config (`signer.network` as operator network).
///
/// Prefer [`client_for_signer_on_network`] when the caller has an explicit program/CLI
/// network (for example `coins-list` before combine context rewrites the signer).
///
/// # Errors
///
/// Returns an error if the signer network is unsupported.
pub fn client_for_signer(signer: &SignerConfig) -> SignerResult<CoinsetClient> {
    client_for_signer_on_network(signer, &signer.network)
}

/// Coinset client for network.
///
/// # Errors
///
/// Returns an error if the network is unsupported.
pub fn client_for_network(network: &str) -> SignerResult<CoinsetClient> {
    direct_coinset_client(network, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chia_sdk_coinset::ChiaRpcClient;

    use crate::coinset::direct_api::TESTNET11_DIRECT_BASE_URL;
    use crate::coinset::DEFAULT_COINSET_BASE_URL;
    use crate::test_support::signer_config::test_signer_config;

    #[test]
    fn client_for_signer_on_testnet11_maps_mainnet_default_url() {
        let signer = test_signer_config(DEFAULT_COINSET_BASE_URL);
        let client = client_for_signer_on_network(&signer, "testnet11").expect("client");
        assert_eq!(client.base_url(), TESTNET11_DIRECT_BASE_URL);
    }
}
