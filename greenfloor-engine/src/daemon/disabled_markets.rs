use std::sync::{Mutex, Once};
use std::time::Instant;

use crate::config::MarketsConfig;
use crate::cycle::{
    next_disabled_market_log_deadline, should_log_disabled_market,
    DEFAULT_DISABLED_MARKET_LOG_INTERVAL_SECONDS, MIN_DISABLED_MARKET_LOG_INTERVAL_SECONDS,
};

static STARTUP_LOGGED: Once = Once::new();
static NEXT_LOG_DEADLINE: std::sync::LazyLock<Mutex<Option<f64>>> =
    std::sync::LazyLock::new(|| Mutex::new(None));

fn monotonic_seconds() -> f64 {
    static START: Once = Once::new();
    static mut ORIGIN: Option<Instant> = None;
    START.call_once(|| unsafe {
        ORIGIN = Some(Instant::now());
    });
    let origin = unsafe { ORIGIN.expect("monotonic origin") };
    origin.elapsed().as_secs_f64()
}

pub fn disabled_market_log_interval_seconds() -> u64 {
    std::env::var("GREENFLOOR_DISABLED_MARKET_LOG_INTERVAL_SECONDS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .map_or(DEFAULT_DISABLED_MARKET_LOG_INTERVAL_SECONDS, |value| {
            value.max(MIN_DISABLED_MARKET_LOG_INTERVAL_SECONDS)
        })
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
        let now = monotonic_seconds();
        if let Ok(mut next_log_deadline) = NEXT_LOG_DEADLINE.lock() {
            *next_log_deadline = Some(next_disabled_market_log_deadline(now, interval_seconds));
        }
    });
}

pub fn log_disabled_markets_periodic(markets: &MarketsConfig) {
    let interval_seconds = disabled_market_log_interval_seconds();
    let now = monotonic_seconds();
    let Ok(mut next_log_deadline) = NEXT_LOG_DEADLINE.lock() else {
        return;
    };
    let deadline = next_log_deadline.unwrap_or(0.0);
    if !should_log_disabled_market(now, deadline) {
        return;
    }
    let disabled_count = markets
        .markets
        .iter()
        .filter(|market| !market.enabled)
        .count();
    if disabled_count == 0 {
        return;
    }
    *next_log_deadline = Some(next_disabled_market_log_deadline(now, interval_seconds));
    tracing::info!(
        count = disabled_count,
        interval_seconds,
        event = "disabled_markets_periodic",
        "disabled_markets"
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
            ladders: HashMap::default(),
        }
    }

    #[test]
    fn disabled_market_log_interval_respects_minimum() {
        std::env::set_var("GREENFLOOR_DISABLED_MARKET_LOG_INTERVAL_SECONDS", "10");
        assert_eq!(
            disabled_market_log_interval_seconds(),
            MIN_DISABLED_MARKET_LOG_INTERVAL_SECONDS
        );
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
