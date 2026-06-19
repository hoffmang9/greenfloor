use serde_json::Value;

use crate::coinset::{resolve_direct_client, DEFAULT_MSP_BASE_URL};
use crate::config::{
    parse_signer_config, program_bundle_gated_from_parsed, ManagerProgramConfig, SignerConfig,
};
use crate::error::SignerResult;

/// Resolved Coinset endpoints for combine scan (direct API) and execution (MSP API).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombineCoinsetContext {
    pub network: String,
    pub direct_base_url: String,
    pub msp_base_url: String,
}

pub fn resolve_combine_coinset_context(
    request_network: Option<&str>,
    coinset_base_url: Option<&str>,
    program_network: &str,
    program_msp_base_url: &str,
) -> CombineCoinsetContext {
    let network_source = request_network
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(program_network);
    let direct = resolve_direct_client(network_source, coinset_base_url);
    let msp_base_url = coinset_base_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map_or_else(
            || program_msp_base_url.trim_end_matches('/').to_string(),
            |url| url.trim_end_matches('/').to_string(),
        );
    CombineCoinsetContext {
        network: direct.network.to_string(),
        direct_base_url: direct.base_url,
        msp_base_url,
    }
}

impl CombineCoinsetContext {
    pub fn program_default_msp_base_url(raw: &Value) -> String {
        parse_signer_config(raw).map_or_else(
            |_| DEFAULT_MSP_BASE_URL.to_string(),
            |cfg| cfg.coinset_msp_base_url,
        )
    }

    pub fn apply_to_execution_signer(&self, mut signer: SignerConfig) -> SignerConfig {
        signer.network.clone_from(&self.network);
        signer.coinset_msp_base_url.clone_from(&self.msp_base_url);
        signer
    }

    pub fn direct_base_url_for_scan(&self) -> &str {
        self.direct_base_url.as_str()
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

    #[test]
    fn resolve_context_normalizes_network_and_splits_direct_vs_msp_urls() {
        let ctx = resolve_combine_coinset_context(
            Some("testnet"),
            Some("https://coinset.custom/"),
            "mainnet",
            "https://api-msp.coinset.org",
        );
        assert_eq!(ctx.network, "testnet11");
        assert_eq!(ctx.direct_base_url, "https://coinset.custom");
        assert_eq!(ctx.msp_base_url, "https://coinset.custom");
    }

    #[test]
    fn resolve_context_uses_program_defaults_without_cli_overrides() {
        let ctx = resolve_combine_coinset_context(None, None, "mainnet", "https://msp.example");
        assert_eq!(ctx.network, "mainnet");
        assert_eq!(ctx.direct_base_url, MAINNET_DIRECT_BASE_URL);
        assert_eq!(ctx.msp_base_url, "https://msp.example");
    }

    #[test]
    fn resolve_context_maps_legacy_coinset_host_to_network_direct_default() {
        let ctx = resolve_combine_coinset_context(
            Some("testnet11"),
            Some("https://coinset.org"),
            "mainnet",
            "https://api-msp.coinset.org",
        );
        assert_eq!(ctx.network, "testnet11");
        assert_eq!(ctx.direct_base_url, TESTNET11_DIRECT_BASE_URL);
        assert_eq!(ctx.msp_base_url, "https://coinset.org");
    }

    #[test]
    fn apply_to_execution_signer_updates_network_and_msp_base_url() {
        let ctx = resolve_combine_coinset_context(
            Some("testnet"),
            Some("https://coinset.custom/"),
            "mainnet",
            "https://api-msp.coinset.org",
        );
        let signer = SignerConfig {
            network: "mainnet".to_string(),
            coinset_msp_base_url: "https://api-msp.coinset.org".to_string(),
            kms_key_id: "key".to_string(),
            kms_region: "us-west-2".to_string(),
            kms_public_key_hex: None,
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
        assert_eq!(got.coinset_msp_base_url, "https://coinset.custom");
    }
}
