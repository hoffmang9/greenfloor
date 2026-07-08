use crate::config::ManagerProgramConfig;
use crate::hex::normalize_hex_id;

pub(crate) fn ensure_rustls_crypto_provider() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Default Coinset WS event filter: transaction lifecycle + offer lifecycle.
pub const DEFAULT_WS_EVENTS: &str = "transaction,offer";
pub const DEFAULT_WS_TX_STATUS: &str = "pending,confirmed";

#[must_use]
pub fn resolve_coinset_ws_base_url(
    program: &ManagerProgramConfig,
    coinset_base_url: &str,
) -> String {
    let configured = program.tx_block_websocket_url.trim();
    if !configured.is_empty() {
        // Strip any operator-supplied query; required filters are always appended below.
        return configured
            .split_once('?')
            .map_or(configured, |(base, _)| base)
            .to_string();
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

/// Append fixed query filters to a Coinset WS base URL.
///
/// Always sets `events` / `tx_status`. When `p2s` is non-empty, each puzzle hash is
/// added as a repeatable `p2` query param.
#[must_use]
pub fn append_coinset_ws_filters(base_ws_url: &str, p2s: &[String]) -> String {
    let trimmed = base_ws_url.trim();
    let base = trimmed.split_once('?').map_or(trimmed, |(bare, _)| bare);
    if base.is_empty() {
        return base.to_string();
    }
    let mut url = format!("{base}?events={DEFAULT_WS_EVENTS}&tx_status={DEFAULT_WS_TX_STATUS}");
    for p2 in p2s {
        let normalized = normalize_hex_id(p2);
        if normalized.len() == 64 {
            url.push_str("&p2=");
            url.push_str(&normalized);
        }
    }
    url
}

/// Resolve the Coinset WS URL with required event filters and stable inventory p2s.
#[must_use]
pub fn resolve_coinset_ws_url_with_p2s(
    program: &ManagerProgramConfig,
    coinset_base_url: &str,
    p2s: &[String],
) -> String {
    let base = resolve_coinset_ws_base_url(program, coinset_base_url);
    append_coinset_ws_filters(&base, p2s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_filters_adds_events_and_p2s() {
        let p2 = "ab".repeat(32);
        let url = append_coinset_ws_filters("wss://api.coinset.org/ws", std::slice::from_ref(&p2));
        assert!(url.starts_with("wss://api.coinset.org/ws?events=transaction,offer"));
        assert!(url.contains("tx_status=pending,confirmed"));
        assert!(url.contains(&format!("p2={p2}")));
    }

    #[test]
    fn configured_url_query_is_replaced_with_required_filters() {
        let program = ManagerProgramConfig {
            tx_block_websocket_url: "wss://example.test/ws?events=peak".to_string(),
            ..Default::default()
        };
        let p2 = "ab".repeat(32);
        let url = resolve_coinset_ws_url_with_p2s(&program, "", std::slice::from_ref(&p2));
        assert!(url.starts_with("wss://example.test/ws?events=transaction,offer"));
        assert!(!url.contains("events=peak"));
        assert!(url.contains(&format!("p2={p2}")));
    }
}
