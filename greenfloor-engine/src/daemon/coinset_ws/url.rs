use crate::config::ManagerProgramConfig;

pub(crate) fn ensure_rustls_crypto_provider() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

#[must_use]
pub fn resolve_coinset_ws_url(program: &ManagerProgramConfig, coinset_base_url: &str) -> String {
    let configured = program.tx_block_websocket_url.trim();
    if !configured.is_empty() {
        return configured.to_string();
    }
    let base_url = coinset_base_url.trim();
    if base_url.is_empty() {
        return if program.network.eq_ignore_ascii_case("testnet11")
            || program.network.eq_ignore_ascii_case("testnet")
        {
            "wss://testnet11.api.coinset.org/ws".to_string()
        } else {
            "wss://api.coinset.org/ws".to_string()
        };
    }
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.starts_with("https://") {
        let host = trimmed.trim_start_matches("https://");
        return format!("wss://{host}/ws");
    }
    if trimmed.starts_with("http://") {
        let host = trimmed.trim_start_matches("http://");
        return format!("ws://{host}/ws");
    }
    trimmed.to_string()
}
