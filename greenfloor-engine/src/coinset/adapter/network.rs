pub const MAINNET_BASE_URL: &str = "https://api.coinset.org";
pub const TESTNET11_BASE_URL: &str = "https://testnet11.api.coinset.org";

pub fn normalize_coinset_network(network: &str) -> &'static str {
    match network.trim().to_ascii_lowercase().as_str() {
        "testnet" | "testnet11" => "testnet11",
        _ => "mainnet",
    }
}

pub fn resolve_coinset_base_url(network: &str, base_url: Option<&str>) -> String {
    let trimmed = base_url.map_or("", str::trim).trim_end_matches('/');
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    if normalize_coinset_network(network) == "testnet11" {
        TESTNET11_BASE_URL.to_string()
    } else {
        MAINNET_BASE_URL.to_string()
    }
}
