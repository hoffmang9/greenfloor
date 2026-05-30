use reqwest::Client;
use serde_json::json;

use crate::coinset::get_all_mempool_tx_ids;
use crate::config::ManagerProgramConfig;
use crate::error::{SignerError, SignerResult};
use crate::storage::SqliteStore;

use super::coinset_ws::capture_coinset_websocket_once;

const DEFAULT_XCH_PRICE_URL: &str = "https://coincodex.com/api/coincodex/get_coin/xch";

#[derive(Debug, Clone, Default)]
pub struct CyclePreambleResult {
    pub cycle_error_count: u64,
    pub xch_price_usd: Option<f64>,
}

pub async fn run_cycle_preamble(
    program: &ManagerProgramConfig,
    store: &SqliteStore,
    coinset_base_url: &str,
    poll_coinset_mempool: bool,
    use_websocket_capture: bool,
) -> SignerResult<CyclePreambleResult> {
    let mut result = CyclePreambleResult::default();

    match fetch_xch_price_usd().await {
        Ok(price) => {
            result.xch_price_usd = Some(price);
            store.add_audit_event("xch_price_snapshot", &json!({"price_usd": price}), None)?;
        }
        Err(err) => {
            result.cycle_error_count += 1;
            store.add_audit_event("xch_price_error", &json!({"error": err.to_string()}), None)?;
        }
    }

    if use_websocket_capture {
        if let Err(err) = capture_coinset_websocket_once(&store, &program, coinset_base_url).await {
            result.cycle_error_count += 1;
            store.add_audit_event(
                "coinset_ws_once_error",
                &json!({"error": err.to_string()}),
                None,
            )?;
        }
    } else if poll_coinset_mempool {
        if let Err(err) = poll_coinset_mempool_snapshot(&store, &program, coinset_base_url).await {
            result.cycle_error_count += 1;
            store.add_audit_event(
                "coinset_mempool_error",
                &json!({"error": err.to_string()}),
                None,
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
    store.add_audit_event(
        "coinset_mempool_snapshot",
        &json!({"count": tx_ids.len()}),
        None,
    )?;
    if new_count > 0 {
        store.add_audit_event(
            "mempool_observed",
            &json!({"new_tx_ids": new_count, "source": "coinset_poll"}),
            None,
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
