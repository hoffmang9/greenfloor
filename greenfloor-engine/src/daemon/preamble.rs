use reqwest::Client;
use serde_json::json;
use tracing::Level;

use crate::coinset::get_all_mempool_tx_ids;
use crate::config::ManagerProgramConfig;
use crate::error::{SignerError, SignerResult};
use crate::operator_log::{
    LogContext, COINSET_MEMPOOL_ERROR, COINSET_MEMPOOL_SNAPSHOT, COINSET_WS_ONCE_ERROR,
    MEMPOOL_OBSERVED, XCH_PRICE_ERROR, XCH_PRICE_SNAPSHOT,
};
use crate::storage::SqliteStore;

use super::coinset_ws::{capture_coinset_websocket_once, CoinsetWsShared};

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
    coinset: &CoinsetWsShared,
    poll_coinset_mempool: bool,
    use_websocket_capture: bool,
) -> SignerResult<CyclePreambleResult> {
    let mut result = CyclePreambleResult::default();

    match fetch_xch_price_usd().await {
        Ok(price) => {
            result.xch_price_usd = Some(price);
            LogContext::DAEMON_CYCLE.dual_audit(
                store,
                Level::INFO,
                "xch price snapshot",
                XCH_PRICE_SNAPSHOT,
                &json!({"price_usd": price}),
                None,
            )?;
        }
        Err(err) => {
            result.cycle_error_count += 1;
            LogContext::DAEMON_CYCLE.dual_audit(
                store,
                Level::WARN,
                "xch price fetch failed",
                XCH_PRICE_ERROR,
                &json!({"error": err.to_string()}),
                None,
            )?;
        }
    }

    if use_websocket_capture {
        if let Err(err) =
            capture_coinset_websocket_once(store, program, coinset_base_url, coinset).await
        {
            result.cycle_error_count += 1;
            LogContext::DAEMON_CYCLE.dual_audit(
                store,
                Level::WARN,
                "coinset websocket capture failed",
                COINSET_WS_ONCE_ERROR,
                &json!({"error": err.to_string()}),
                None,
            )?;
        }
    } else if poll_coinset_mempool {
        if let Err(err) = poll_coinset_mempool_snapshot(store, program, coinset_base_url).await {
            result.cycle_error_count += 1;
            LogContext::DAEMON_CYCLE.dual_audit(
                store,
                Level::WARN,
                "coinset mempool poll failed",
                COINSET_MEMPOOL_ERROR,
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
    let new_count = store.ingest_tx_signals(&tx_ids, crate::storage::TxSignalIngress::Mempool)?;
    LogContext::DAEMON_CYCLE.dual_audit(
        store,
        Level::DEBUG,
        "coinset mempool snapshot",
        COINSET_MEMPOOL_SNAPSHOT,
        &json!({"count": tx_ids.len()}),
        None,
    )?;
    if new_count > 0 {
        LogContext::DAEMON_CYCLE.dual_audit(
            store,
            Level::INFO,
            "mempool txs observed",
            MEMPOOL_OBSERVED,
            &json!({"new_tx_ids": new_count, "source": "coinset_poll"}),
            None,
        )?;
    }
    Ok(())
}

async fn fetch_xch_price_usd() -> SignerResult<f64> {
    if let Some(price) = xch_price_from_env_override()? {
        return Ok(price);
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

pub(crate) fn xch_price_from_env_override() -> SignerResult<Option<f64>> {
    let Ok(raw) = std::env::var("GREENFLOOR_XCH_PRICE_USD") else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let price: f64 = trimmed
        .parse()
        .map_err(|err| SignerError::Other(format!("invalid GREENFLOOR_XCH_PRICE_USD: {err}")))?;
    if price > 0.0 {
        Ok(Some(price))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::test_support::{open_test_store, sample_mainnet_program};
    use crate::operator_log::XCH_PRICE_SNAPSHOT;
    use crate::test_env::EnvRestoreGuard;

    #[test]
    fn xch_price_from_env_override_parses_positive_values() {
        let _env = EnvRestoreGuard::set(&[("GREENFLOOR_XCH_PRICE_USD", "42.5")]);
        assert_eq!(xch_price_from_env_override().expect("price"), Some(42.5));
    }

    #[test]
    fn xch_price_from_env_override_rejects_invalid_values() {
        let _env = EnvRestoreGuard::set(&[("GREENFLOOR_XCH_PRICE_USD", "not-a-number")]);
        let err = xch_price_from_env_override().expect_err("invalid");
        assert!(err.to_string().contains("invalid GREENFLOOR_XCH_PRICE_USD"));
    }

    #[tokio::test]
    async fn run_cycle_preamble_records_xch_price_snapshot_when_env_set() {
        let _env = EnvRestoreGuard::set(&[("GREENFLOOR_XCH_PRICE_USD", "42.5")]);
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_test_store(&dir.path().join("state.sqlite"));

        let result = run_cycle_preamble(
            &sample_mainnet_program(),
            &store,
            "",
            &CoinsetWsShared::empty(),
            false,
            false,
        )
        .await
        .expect("preamble");

        assert_eq!(result.xch_price_usd, Some(42.5));
        assert_eq!(result.cycle_error_count, 0);
        let events = store
            .list_recent_audit_events(Some(&[XCH_PRICE_SNAPSHOT]), None, 1)
            .expect("audit");
        assert_eq!(events[0].payload.get("price_usd"), Some(&json!(42.5)));
    }

    #[tokio::test]
    async fn run_cycle_preamble_mempool_poll_failure_increments_cycle_errors() {
        let _env = EnvRestoreGuard::set(&[("GREENFLOOR_XCH_PRICE_USD", "33.0")]);
        let dir = tempfile::tempdir().expect("tempdir");
        let store = open_test_store(&dir.path().join("state.sqlite"));

        let result = run_cycle_preamble(
            &sample_mainnet_program(),
            &store,
            "http://127.0.0.1:1",
            &CoinsetWsShared::empty(),
            true,
            false,
        )
        .await
        .expect("preamble");

        assert_eq!(result.cycle_error_count, 1);
    }
}
