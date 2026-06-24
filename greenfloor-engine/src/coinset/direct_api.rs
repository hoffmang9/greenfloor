//! Direct Coinset.org HTTP hosts for generic RPC used by `coinset_cli` and vault scan scripts.

pub const MAINNET_DIRECT_BASE_URL: &str = "https://api.coinset.org";
pub const TESTNET11_DIRECT_BASE_URL: &str = "https://testnet11.api.coinset.org";

const LEGACY_MAINNET_HOST_ALIASES: &[&str] = &[
    "coinset.org",
    "https://coinset.org",
    "http://coinset.org",
    "www.coinset.org",
    "https://www.coinset.org",
    "http://www.coinset.org",
];

const LEGACY_TESTNET11_HOST_ALIASES: &[&str] = &[
    "testnet11.coinset.org",
    "https://testnet11.coinset.org",
    "http://testnet11.coinset.org",
    "www.testnet11.coinset.org",
    "https://www.testnet11.coinset.org",
    "http://www.testnet11.coinset.org",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDirectClient {
    pub network: &'static str,
    pub base_url: String,
}

#[must_use]
pub fn normalize_coinset_network(network: &str) -> &'static str {
    match network.trim().to_ascii_lowercase().as_str() {
        "testnet" | "testnet11" => "testnet11",
        _ => "mainnet",
    }
}

pub fn is_legacy_coinset_host_alias(url: &str) -> bool {
    let lower = url.trim().trim_end_matches('/').to_ascii_lowercase();
    LEGACY_MAINNET_HOST_ALIASES
        .iter()
        .any(|alias| lower == *alias)
        || LEGACY_TESTNET11_HOST_ALIASES
            .iter()
            .any(|alias| lower == *alias)
}

pub fn explicit_coinset_url_override(base_url: Option<&str>) -> Option<&str> {
    let raw = base_url.map(str::trim).filter(|value| !value.is_empty())?;
    if is_legacy_coinset_host_alias(raw) {
        return None;
    }
    Some(raw)
}

#[must_use]
pub fn normalize_direct_base_url_input(base_url: Option<&str>) -> Option<&str> {
    explicit_coinset_url_override(base_url)
}

#[must_use]
pub fn resolve_direct_coinset_base_url(network: &str, base_url: Option<&str>) -> String {
    if let Some(url) = normalize_direct_base_url_input(base_url) {
        return url.trim_end_matches('/').to_string();
    }
    if normalize_coinset_network(network) == "testnet11" {
        TESTNET11_DIRECT_BASE_URL.to_string()
    } else {
        MAINNET_DIRECT_BASE_URL.to_string()
    }
}

#[must_use]
pub fn resolve_direct_client(network: &str, base_url: Option<&str>) -> ResolvedDirectClient {
    let network = normalize_coinset_network(network);
    ResolvedDirectClient {
        network,
        base_url: resolve_direct_coinset_base_url(network, base_url),
    }
}

fn canonical_coinset_base_url_mismatches_network(configured_url: &str, network: &str) -> bool {
    match network {
        "testnet11" => configured_url == MAINNET_DIRECT_BASE_URL,
        "mainnet" => configured_url == TESTNET11_DIRECT_BASE_URL,
        _ => false,
    }
}

/// Pick the Coinset base URL for an operator network and configured signer value.
///
/// Empty configured URLs and canonical defaults for the wrong network fall back to the
/// network-native direct Coinset host.
#[must_use]
pub fn effective_coinset_base_url(network: &str, configured_url: &str) -> String {
    let network = normalize_coinset_network(network);
    let configured = configured_url.trim().trim_end_matches('/');
    if configured.is_empty() {
        return resolve_direct_coinset_base_url(network, None);
    }
    if canonical_coinset_base_url_mismatches_network(configured, network) {
        return resolve_direct_coinset_base_url(network, None);
    }
    configured.to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        effective_coinset_base_url, explicit_coinset_url_override, is_legacy_coinset_host_alias,
        normalize_coinset_network, normalize_direct_base_url_input, resolve_direct_client,
        resolve_direct_coinset_base_url, MAINNET_DIRECT_BASE_URL, TESTNET11_DIRECT_BASE_URL,
    };

    #[test]
    fn normalize_coinset_network_maps_testnet_aliases() {
        assert_eq!(normalize_coinset_network("testnet"), "testnet11");
        assert_eq!(normalize_coinset_network("testnet11"), "testnet11");
        assert_eq!(normalize_coinset_network("mainnet"), "mainnet");
        assert_eq!(normalize_coinset_network("unknown"), "mainnet");
    }

    #[test]
    fn explicit_coinset_url_override_rejects_legacy_hosts() {
        assert_eq!(
            explicit_coinset_url_override(Some("https://coinset.org/")),
            None
        );
        assert_eq!(
            explicit_coinset_url_override(Some("https://coinset.custom")),
            Some("https://coinset.custom")
        );
    }

    #[test]
    fn legacy_host_aliases_map_to_defaults() {
        assert!(is_legacy_coinset_host_alias("https://coinset.org"));
        assert!(is_legacy_coinset_host_alias("testnet11.coinset.org"));
        assert_eq!(
            normalize_direct_base_url_input(Some("https://coinset.org/")),
            None
        );
        assert_eq!(
            resolve_direct_coinset_base_url("mainnet", Some("https://coinset.org")),
            MAINNET_DIRECT_BASE_URL
        );
    }

    #[test]
    fn resolve_direct_coinset_base_url_defaults_by_network() {
        assert_eq!(
            resolve_direct_coinset_base_url("mainnet", None),
            MAINNET_DIRECT_BASE_URL
        );
        assert_eq!(
            resolve_direct_coinset_base_url("testnet11", None),
            TESTNET11_DIRECT_BASE_URL
        );
        assert_eq!(
            resolve_direct_coinset_base_url("testnet", None),
            TESTNET11_DIRECT_BASE_URL
        );
        assert_eq!(
            resolve_direct_coinset_base_url("testnet11", Some("https://coinset.custom")),
            "https://coinset.custom"
        );
    }

    #[test]
    fn resolve_direct_client_normalizes_network_and_url() {
        let resolved = resolve_direct_client("testnet", Some("https://coinset.org"));
        assert_eq!(resolved.network, "testnet11");
        assert_eq!(resolved.base_url, TESTNET11_DIRECT_BASE_URL);
    }

    #[test]
    fn effective_coinset_base_url_uses_network_default_when_configured_empty() {
        assert_eq!(
            effective_coinset_base_url("testnet11", ""),
            TESTNET11_DIRECT_BASE_URL
        );
    }

    #[test]
    fn effective_coinset_base_url_replaces_mainnet_default_on_testnet11() {
        assert_eq!(
            effective_coinset_base_url("testnet11", MAINNET_DIRECT_BASE_URL),
            TESTNET11_DIRECT_BASE_URL
        );
    }

    #[test]
    fn effective_coinset_base_url_honors_custom_configured_url() {
        assert_eq!(
            effective_coinset_base_url("testnet11", "https://coinset.custom"),
            "https://coinset.custom"
        );
    }
}
