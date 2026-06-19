use reqwest::Client;
use serde_json::{json, Value};
use tracing::Level;

use crate::coinset::get_all_mempool_tx_ids;
use crate::config::ManagerProgramConfig;
use crate::error::{SignerError, SignerResult};
use crate::operator_log::{
    audit_daemon_cycle, COINSET_MEMPOOL_ERROR, COINSET_MEMPOOL_SNAPSHOT, COINSET_WS_ONCE_ERROR,
    MEMPOOL_OBSERVED, XCH_PRICE_ERROR, XCH_PRICE_SNAPSHOT,
};
use crate::storage::SqliteStore;

use super::coinset_ws::capture_coinset_websocket_once;
use super::watchlist::cache::CoinWatchlistCache;

const DEFAULT_XCH_PRICE_URL: &str = "https://coincodex.com/api/coincodex/get_coin/xch";

#[derive(Debug, Clone, Default)]
pub struct CyclePreambleResult {
    pub cycle_error_count: u64,
    pub xch_price_usd: Option<f64>,
}

fn audit_preamble(
    store: &SqliteStore,
    level: Level,
    event: &str,
    payload: &Value,
    trace_message: &'static str,
) -> SignerResult<()> {
    audit_daemon_cycle(store, level, event, payload, trace_message)
}

pub async fn run_cycle_preamble(
    program: &ManagerProgramConfig,
    store: &SqliteStore,
    coinset_base_url: &str,
    coin_watchlist: &CoinWatchlistCache,
    poll_coinset_mempool: bool,
    use_websocket_capture: bool,
) -> SignerResult<CyclePreambleResult> {
    let mut result = CyclePreambleResult::default();

    match fetch_xch_price_usd().await {
        Ok(price) => {
            result.xch_price_usd = Some(price);
            audit_preamble(
                store,
                Level::INFO,
                XCH_PRICE_SNAPSHOT,
                &json!({"price_usd": price}),
                "xch price snapshot",
            )?;
        }
        Err(err) => {
            result.cycle_error_count += 1;
            audit_preamble(
                store,
                Level::WARN,
                XCH_PRICE_ERROR,
                &json!({"error": err.to_string()}),
                "xch price fetch failed",
            )?;
        }
    }

    if use_websocket_capture {
        if let Err(err) =
            capture_coinset_websocket_once(store, program, coinset_base_url, coin_watchlist).await
        {
            result.cycle_error_count += 1;
            audit_preamble(
                store,
                Level::WARN,
                COINSET_WS_ONCE_ERROR,
                &json!({"error": err.to_string()}),
                "coinset websocket capture failed",
            )?;
        }
    } else if poll_coinset_mempool {
        if let Err(err) = poll_coinset_mempool_snapshot(store, program, coinset_base_url).await {
            result.cycle_error_count += 1;
            audit_preamble(
                store,
                Level::WARN,
                COINSET_MEMPOOL_ERROR,
                &json!({"error": err.to_string()}),
                "coinset mempool poll failed",
            )?;
        }
    }

    Ok(result)
}

async fn poll_coinset_mempool_snapshot(
    store: &SqliteStore,
    program: &ManagerProgramConfig,
    coinset_base_url: &str,
) -> SignerResult<()> {
    let base_url = coinset_base_url.trim();
    let base_opt = if base_url.is_empty() {
        None
    } else {
        Some(base_url)
    };
    let tx_ids = get_all_mempool_tx_ids(&program.network, base_opt).await?;
    let new_count = store.observe_mempool_tx_ids(&tx_ids)?;
    audit_preamble(
        store,
        Level::DEBUG,
        COINSET_MEMPOOL_SNAPSHOT,
        &json!({"count": tx_ids.len()}),
        "coinset mempool snapshot",
    )?;
    if new_count > 0 {
        audit_preamble(
            store,
            Level::INFO,
            MEMPOOL_OBSERVED,
            &json!({"new_tx_ids": new_count, "source": "coinset_poll"}),
            "mempool txs observed",
        )?;
    }
    Ok(())
}

async fn fetch_xch_price_usd() -> SignerResult<f64> {
    if let Ok(raw) = std::env::var("GREENFLOOR_XCH_PRICE_USD") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let price: f64 = trimmed.parse().map_err(|err| {
                SignerError::Other(format!("invalid GREENFLOOR_XCH_PRICE_USD: {err}"))
            })?;
            if price > 0.0 {
                return Ok(price);
            }
        }
    }
    let url = std::env::var("GREENFLOOR_XCH_PRICE_URL")
        .unwrap_or_else(|_| DEFAULT_XCH_PRICE_URL.to_string());
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|err| SignerError::Other(format!("xch_price_client_error:{err}")))?;
    let response = client
        .get(url.trim())
        .send()
        .await
        .map_err(|err| SignerError::Other(format!("xch_price_fetch_error:{err}")))?;
    let payload: serde_json::Value = response
        .json()
        .await
        .map_err(|err| SignerError::Other(format!("xch_price_decode_error:{err}")))?;
    if let Some(price) = payload
        .get("last_price_usd")
        .and_then(serde_json::Value::as_f64)
    {
        if price > 0.0 {
            return Ok(price);
        }
    }
    if let Some(items) = payload.as_array() {
        if let Some(first) = items.first() {
            if let Some(price) = first
                .get("current_price")
                .and_then(serde_json::Value::as_f64)
            {
                if price > 0.0 {
                    return Ok(price);
                }
            }
        }
    }
    Err(SignerError::Other("xch_price_unavailable".to_string()))
}
