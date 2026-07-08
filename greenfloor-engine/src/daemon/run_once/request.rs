use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::daemon::coinset_ws::CoinsetProcessContext;

#[cfg(test)]
use crate::daemon::dispatch_test_controls::DaemonDispatchTestInjections;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DaemonDispatchState {
    pub cursor: usize,
    pub immediate_requeue_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonCycleTestControls {
    #[serde(default)]
    pub skip_strategy_execution: bool,
    #[serde(default)]
    pub force_market_error_for: Option<String>,
    #[cfg(test)]
    #[serde(default, skip)]
    pub offer_dispatch: DaemonDispatchTestInjections,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonRunOnceRequest {
    pub program_path: PathBuf,
    pub markets_path: PathBuf,
    #[serde(default)]
    pub testnet_markets_path: Option<PathBuf>,
    #[serde(default)]
    pub state_db_override: Option<String>,
    pub coinset_base_url: String,
    pub state_dir: PathBuf,
    #[serde(default = "default_poll_coinset_mempool")]
    pub poll_coinset_mempool: bool,
    #[serde(default)]
    pub use_websocket_capture: bool,
    #[serde(default)]
    pub allowed_key_ids: Vec<String>,
    #[serde(default)]
    pub dispatch_state: DaemonDispatchState,
    #[serde(default)]
    pub test_controls: DaemonCycleTestControls,
    #[serde(skip, default = "default_coinset_process_context")]
    pub coinset: Arc<CoinsetProcessContext>,
}

fn default_poll_coinset_mempool() -> bool {
    true
}

fn default_coinset_process_context() -> Arc<CoinsetProcessContext> {
    CoinsetProcessContext::empty()
}

impl DaemonRunOnceRequest {
    /// From json value.
    ///
    /// Builds `coinset` from markets paths (same contract as CLI `--once` / daemon loop).
    ///
    /// # Errors
    ///
    /// Returns an error if JSON parse fails or inventory p2s cannot be derived from markets.
    pub fn from_json_value(value: Value) -> crate::error::SignerResult<Self> {
        let mut request: Self = serde_json::from_value(value)
            .map_err(|err| crate::error::SignerError::Other(err.to_string()))?;
        request.coinset = CoinsetProcessContext::from_markets(
            &request.markets_path,
            request.testnet_markets_path.as_deref(),
        )?;
        Ok(request)
    }
}
