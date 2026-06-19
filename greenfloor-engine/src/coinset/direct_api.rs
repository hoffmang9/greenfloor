//! Direct Coinset.org HTTP hosts for generic RPC used by `coinset_cli` and vault scan scripts.

pub const MAINNET_DIRECT_BASE_URL: &str = "https://api.coinset.org";
pub const TESTNET11_DIRECT_BASE_URL: &str = "https://testnet11.api.coinset.org";

pub fn normalize_coinset_network(network: &str) -> &'static str {
    match network.trim().to_ascii_lowercase().as_str() {
        "testnet" | "testnet11" => "testnet11",
        _ => "mainnet",
    }
}

pub fn resolve_direct_coinset_base_url(network: &str, base_url: Option<&str>) -> String {
    if let Some(url) = base_url.map(str::trim).filter(|value| !value.is_empty()) {
        return url.trim_end_matches('/').to_string();
    }
    if normalize_coinset_network(network) == "testnet11" {
        TESTNET11_DIRECT_BASE_URL.to_string()
    } else {
        MAINNET_DIRECT_BASE_URL.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_coinset_network, resolve_direct_coinset_base_url, MAINNET_DIRECT_BASE_URL,
        TESTNET11_DIRECT_BASE_URL,
    };

    #[test]
    fn normalize_coinset_network_maps_testnet_aliases() {
        assert_eq!(normalize_coinset_network("testnet"), "testnet11");
        assert_eq!(normalize_coinset_network("testnet11"), "testnet11");
        assert_eq!(normalize_coinset_network("mainnet"), "mainnet");
        assert_eq!(normalize_coinset_network("unknown"), "mainnet");
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
}
