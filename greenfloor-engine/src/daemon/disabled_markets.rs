use std::collections::HashMap;
use std::sync::{Mutex, Once};
use std::time::{Duration, Instant};

use crate::config::MarketsConfig;

const DEFAULT_LOG_INTERVAL_SECONDS: u64 = 3600;
const MIN_LOG_INTERVAL_SECONDS: u64 = 60;

static STARTUP_LOGGED: Once = Once::new();
static NEXT_LOG_AT: std::sync::LazyLock<Mutex<HashMap<String, Instant>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn disabled_market_log_interval_seconds() -> u64 {
    std::env::var("GREENFLOOR_DISABLED_MARKET_LOG_INTERVAL_SECONDS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .map(|value| value.max(MIN_LOG_INTERVAL_SECONDS))
        .unwrap_or(DEFAULT_LOG_INTERVAL_SECONDS)
}

pub fn log_disabled_markets_startup_once(markets: &MarketsConfig) {
    STARTUP_LOGGED.call_once(|| {
        let interval_seconds = disabled_market_log_interval_seconds();
        let mut disabled_market_ids: Vec<String> = markets
            .markets
            .iter()
            .filter(|market| !market.enabled)
            .map(|market| market.market_id.trim().to_string())
            .filter(|market_id| !market_id.is_empty())
            .collect();
        disabled_market_ids.sort();
        disabled_market_ids.dedup();
        if disabled_market_ids.is_empty() {
            return;
        }
        tracing::info!(
            count = disabled_market_ids.len(),
            interval_seconds,
            market_ids = ?disabled_market_ids,
            "disabled_markets_startup"
        );
        let throttle_until =
            Instant::now() + Duration::from_secs(interval_seconds);
        if let Ok(mut next_log_at) = NEXT_LOG_AT.lock() {
            for market_id in disabled_market_ids {
                next_log_at.insert(market_id, throttle_until);
            }
        }
    });
}

pub fn log_disabled_market_skip(market_id: &str) {
    let market_id = market_id.trim();
    if market_id.is_empty() {
        return;
    }
    let now = Instant::now();
    let Ok(mut next_log_at) = NEXT_LOG_AT.lock() else {
        return;
    };
    let allowed = next_log_at
        .get(market_id)
        .is_none_or(|deadline| now >= *deadline);
    if !allowed {
        return;
    }
    next_log_at.insert(
        market_id.to_string(),
        now + Duration::from_secs(disabled_market_log_interval_seconds()),
    );
    tracing::info!(
        market_id,
        event = "market_skipped",
        reason = "disabled",
        "market_decision"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MarketConfig;
    use serde_json::json;
    use std::collections::HashMap;

    fn sample_market(market_id: &str, enabled: bool) -> MarketConfig {
        MarketConfig {
            market_id: market_id.to_string(),
            enabled,
            base_asset: "asset1".to_string(),
            base_symbol: "AS1".to_string(),
            quote_asset: "xch".to_string(),
            quote_asset_type: "unstable".to_string(),
            receive_address: "xch1test".to_string(),
            signer_key_id: "key-1".to_string(),
            mode: "sell_only".to_string(),
            pricing: json!({}),
            cancel_move_threshold_bps: None,
            ladders: HashMap::new(),
        }
    }

    #[test]
    fn disabled_market_log_interval_respects_minimum() {
        std::env::set_var("GREENFLOOR_DISABLED_MARKET_LOG_INTERVAL_SECONDS", "10");
        assert_eq!(disabled_market_log_interval_seconds(), MIN_LOG_INTERVAL_SECONDS);
        std::env::remove_var("GREENFLOOR_DISABLED_MARKET_LOG_INTERVAL_SECONDS");
    }

    #[test]
    fn startup_log_is_idempotent_for_disabled_markets() {
        let markets = MarketsConfig {
            markets: vec![
                sample_market("enabled", true),
                sample_market("disabled-a", false),
            ],
        };
        log_disabled_markets_startup_once(&markets);
        log_disabled_markets_startup_once(&markets);
    }
}
