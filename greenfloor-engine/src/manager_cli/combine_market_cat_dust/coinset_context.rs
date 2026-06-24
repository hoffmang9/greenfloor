use serde_json::Value;

use crate::coinset::{
    direct_coinset_client, resolve_coinset_endpoint, CoinsetClient, DEFAULT_COINSET_BASE_URL,
};
use crate::config::{
    parse_signer_config, program_bundle_gated_from_parsed, ManagerProgramConfig, SignerConfig,
};
use crate::error::SignerResult;

/// Resolved Coinset HTTP endpoint for combine scan, lineage preflight, and execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombineCoinsetContext {
    pub network: String,
    pub base_url: String,
}

pub fn resolve_combine_coinset_context(
    request_network: Option<&str>,
    coinset_base_url: Option<&str>,
    program_network: &str,
    program_coinset_base_url: &str,
) -> CombineCoinsetContext {
    let network_source = request_network
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(program_network);
    let endpoint =
        resolve_coinset_endpoint(network_source, program_coinset_base_url, coinset_base_url);
    CombineCoinsetContext {
        network: endpoint.network.to_string(),
        base_url: endpoint.base_url,
    }
}

impl CombineCoinsetContext {
    pub fn program_default_coinset_base_url(raw: &Value) -> String {
        parse_signer_config(raw).map_or_else(
            |_| DEFAULT_COINSET_BASE_URL.to_string(),
            |cfg| cfg.coinset_base_url,
        )
    }

    pub fn apply_to_execution_signer(&self, mut signer: SignerConfig) -> SignerConfig {
        signer.network.clone_from(&self.network);
        signer.coinset_base_url.clone_from(&self.base_url);
        signer
    }

    pub fn base_url(&self) -> &str {
        self.base_url.as_str()
    }

    /// Coinset client for scan, lineage preflight, and execution on this command.
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be constructed.
    pub fn client(&self) -> SignerResult<CoinsetClient> {
        direct_coinset_client(&self.network, Some(self.base_url()))
    }
}

pub(crate) fn load_execution_signer(
    raw: &Value,
    program: ManagerProgramConfig,
    coinset_ctx: &CombineCoinsetContext,
) -> SignerResult<SignerConfig> {
    let bundle = program_bundle_gated_from_parsed(program, raw)?;
    Ok(coinset_ctx.apply_to_execution_signer(bundle.signer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coinset::{MAINNET_DIRECT_BASE_URL, TESTNET11_DIRECT_BASE_URL};
    use crate::vault::context::VaultCustodySnapshot;
    use chia_sdk_coinset::ChiaRpcClient;

    #[test]
    fn resolve_context_normalizes_network_and_cli_override() {
        let ctx = resolve_combine_coinset_context(
            Some("testnet"),
            Some("https://coinset.custom/"),
            "mainnet",
            "https://api.coinset.org",
        );
        assert_eq!(ctx.network, "testnet11");
        assert_eq!(ctx.base_url, "https://coinset.custom");
    }

    #[test]
    fn resolve_context_uses_program_default_without_cli_override() {
        let ctx = resolve_combine_coinset_context(None, None, "mainnet", "https://coinset.example");
        assert_eq!(ctx.network, "mainnet");
        assert_eq!(ctx.base_url, "https://coinset.example");
    }

    #[test]
    fn resolve_context_maps_legacy_coinset_host_to_network_direct_default() {
        let ctx = resolve_combine_coinset_context(
            Some("testnet11"),
            Some("https://coinset.org"),
            "mainnet",
            "https://api.coinset.org",
        );
        assert_eq!(ctx.network, "testnet11");
        assert_eq!(ctx.base_url, TESTNET11_DIRECT_BASE_URL);
    }

    #[test]
    fn resolve_context_uses_testnet11_coinset_when_program_has_mainnet_default() {
        let ctx = resolve_combine_coinset_context(
            Some("testnet11"),
            None,
            "mainnet",
            DEFAULT_COINSET_BASE_URL,
        );
        assert_eq!(ctx.network, "testnet11");
        assert_eq!(ctx.base_url, TESTNET11_DIRECT_BASE_URL);
    }

    #[test]
    fn resolve_context_honors_custom_program_coinset_on_testnet11() {
        let ctx = resolve_combine_coinset_context(
            Some("testnet11"),
            None,
            "mainnet",
            "https://coinset.custom",
        );
        assert_eq!(ctx.network, "testnet11");
        assert_eq!(ctx.base_url, "https://coinset.custom");
    }

    #[test]
    fn resolve_context_maps_legacy_mainnet_host_to_program_default() {
        let ctx = resolve_combine_coinset_context(
            None,
            Some("https://coinset.org"),
            "mainnet",
            "https://api.coinset.org",
        );
        assert_eq!(ctx.network, "mainnet");
        assert_eq!(ctx.base_url, "https://api.coinset.org");
    }

    #[test]
    fn apply_to_execution_signer_updates_network_and_coinset_base_url() {
        let ctx = resolve_combine_coinset_context(
            Some("testnet"),
            Some("https://coinset.custom/"),
            "mainnet",
            "https://api.coinset.org",
        );
        let signer = SignerConfig {
            network: "mainnet".to_string(),
            coinset_base_url: "https://api.coinset.org".to_string(),
            kms_key_id: "key".to_string(),
            kms_region: "us-west-2".to_string(),
            kms_public_key_hex: None,
            kms_runtime: crate::kms::KmsRuntime::default(),
            vault: VaultCustodySnapshot {
                launcher_id: chia_protocol::Bytes32::default(),
                custody_threshold: 1,
                recovery_threshold: 1,
                recovery_clawback_timelock: 3600,
                custody_keys: Vec::new(),
                recovery_keys: Vec::new(),
            },
        };
        let got = ctx.apply_to_execution_signer(signer);
        assert_eq!(got.network, "testnet11");
        assert_eq!(got.coinset_base_url, "https://coinset.custom");
    }

    #[test]
    fn resolve_context_falls_back_to_network_direct_default_when_program_empty() {
        let ctx = resolve_combine_coinset_context(None, None, "testnet11", "");
        assert_eq!(ctx.network, "testnet11");
        assert_eq!(ctx.base_url, TESTNET11_DIRECT_BASE_URL);
    }

    #[test]
    fn client_uses_resolved_base_url() {
        let ctx = resolve_combine_coinset_context(
            Some("mainnet"),
            Some("https://coinset.custom/"),
            "mainnet",
            MAINNET_DIRECT_BASE_URL,
        );
        let client = ctx.client().expect("client");
        assert_eq!(client.base_url(), "https://coinset.custom");
    }
}
