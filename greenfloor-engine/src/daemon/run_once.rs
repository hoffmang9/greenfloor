use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cycle::{
    dedupe_sorted_market_ids, enqueue_immediate_requeue, select_market_batch,
    should_use_market_slot_dispatch, StaleSweepProgress,
};
use crate::daemon::watchlist::cache::CoinWatchlistCache;
use crate::error::SignerResult;
use crate::metrics::{millis_to_u64, non_negative_u64_to_usize};
use crate::storage::{resolve_state_db_path, SqliteStore};

use super::market_context::DaemonCycleResources;
use super::markets::enabled_market_ids;
use super::stale_sweep::detect_stale_open_offers_for_requeue;

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
}

/// Env gate for non-default `test_controls` on `greenfloor-engine daemon-once`.
pub fn daemon_test_controls_enabled() -> bool {
    std::env::var("GREENFLOOR_DAEMON_TEST_CONTROLS")
        .ok()
        .is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        })
}

impl DaemonCycleTestControls {
    pub fn is_non_default(&self) -> bool {
        self.skip_strategy_execution || self.force_market_error_for.is_some()
    }

    pub fn ensure_allowed(&self) -> SignerResult<()> {
        if self.is_non_default() && !daemon_test_controls_enabled() {
            return Err(crate::error::SignerError::Other(
                "non-default daemon test_controls require GREENFLOOR_DAEMON_TEST_CONTROLS=1"
                    .to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonRunOnceRequest {
    pub program_path: PathBuf,
    pub markets_path: PathBuf,
    pub testnet_markets_path: Option<PathBuf>,
    pub state_db_override: Option<String>,
    pub coinset_base_url: String,
    pub state_dir: PathBuf,
    pub poll_coinset_mempool: bool,
    pub use_websocket_capture: bool,
    pub allowed_key_ids: Vec<String>,
    #[serde(default)]
    pub dispatch_state: DaemonDispatchState,
    #[serde(default)]
    pub test_controls: DaemonCycleTestControls,
    #[serde(skip)]
    pub coin_watchlist: Arc<CoinWatchlistCache>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonRunOnceRequestBody {
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
}

fn default_poll_coinset_mempool() -> bool {
    true
}

impl DaemonRunOnceRequestBody {
    pub fn into_engine(self, coin_watchlist: Arc<CoinWatchlistCache>) -> DaemonRunOnceRequest {
        DaemonRunOnceRequest {
            program_path: self.program_path,
            markets_path: self.markets_path,
            testnet_markets_path: self.testnet_markets_path,
            state_db_override: self.state_db_override,
            coinset_base_url: self.coinset_base_url,
            state_dir: self.state_dir,
            poll_coinset_mempool: self.poll_coinset_mempool,
            use_websocket_capture: self.use_websocket_capture,
            allowed_key_ids: self.allowed_key_ids,
            dispatch_state: self.dispatch_state,
            test_controls: self.test_controls,
            coin_watchlist,
        }
    }
}

impl DaemonRunOnceRequest {
    pub fn from_json_value(
        value: Value,
        coin_watchlist: Arc<CoinWatchlistCache>,
    ) -> SignerResult<Self> {
        let body: DaemonRunOnceRequestBody = serde_json::from_value(value)
            .map_err(|err| crate::error::SignerError::Other(err.to_string()))?;
        Ok(body.into_engine(coin_watchlist))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CyclePlan {
    pub enabled_market_ids: Vec<String>,
    pub selected_market_ids: Vec<String>,
    pub consumed_immediate_requeues: Vec<String>,
    pub dispatch_state: DaemonDispatchState,
    pub stale_open_sweep: StaleSweepProgress,
    pub configured_market_slot_count: u64,
    pub runtime_dry_run: bool,
    pub db_path: PathBuf,
    pub previous_xch_price_usd: Option<f64>,
    pub dexie_base_url: String,
    pub splash_base_url: String,
    pub test_controls: DaemonCycleTestControls,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MarketDispatchMetrics {
    pub markets_processed: u64,
    pub cycle_error_count: u64,
    pub strategy_planned_total: u64,
    pub strategy_executed_total: u64,
    pub cancel_triggered_count: u64,
    pub cancel_planned_total: u64,
    pub cancel_executed_total: u64,
    pub immediate_requeue_market_ids: Vec<String>,
}

pub async fn build_cycle_plan(
    request: &DaemonRunOnceRequest,
    resources: &DaemonCycleResources,
    store: &SqliteStore,
) -> SignerResult<CyclePlan> {
    let program = resources.program();
    let db_path = resolve_state_db_path(&program.home_dir, request.state_db_override.as_deref());
    let previous_xch_price_usd = store.get_latest_xch_price_snapshot()?;
    let enabled_market_ids = enabled_market_ids(&resources.markets);

    let stale_open_sweep = if enabled_market_ids.is_empty() {
        StaleSweepProgress {
            checked_offer_count: 0,
            requeue_market_ids: Vec::new(),
            hits: Vec::new(),
            truncated: false,
        }
    } else {
        detect_stale_open_offers_for_requeue(store, &resources.dexie, &enabled_market_ids).await?
    };

    let runtime_market_slot_count = program.runtime_market_slot_count;
    let runtime_dry_run = program.runtime_dry_run;
    let mut dispatch_state = request.dispatch_state.clone();
    for market_id in &stale_open_sweep.requeue_market_ids {
        dispatch_state.immediate_requeue_ids =
            enqueue_immediate_requeue(&dispatch_state.immediate_requeue_ids, market_id);
    }

    let (selected_market_ids, consumed_immediate_requeues) = if should_use_market_slot_dispatch(
        enabled_market_ids.len(),
        non_negative_u64_to_usize(runtime_market_slot_count),
    ) {
        let selection = select_market_batch(
            &enabled_market_ids,
            non_negative_u64_to_usize(runtime_market_slot_count),
            dispatch_state.cursor,
            &dispatch_state.immediate_requeue_ids,
        );
        dispatch_state.cursor = selection.cursor;
        dispatch_state.immediate_requeue_ids = selection.immediate_requeue_ids;
        (
            selection.selected_market_ids,
            selection.consumed_immediate_requeues,
        )
    } else {
        if !enabled_market_ids.is_empty() {
            dispatch_state.cursor %= enabled_market_ids.len();
        }
        (enabled_market_ids.clone(), Vec::new())
    };

    Ok(CyclePlan {
        enabled_market_ids,
        selected_market_ids,
        consumed_immediate_requeues,
        dispatch_state,
        stale_open_sweep,
        configured_market_slot_count: runtime_market_slot_count,
        runtime_dry_run,
        db_path,
        previous_xch_price_usd,
        dexie_base_url: program.dexie_api_base.clone(),
        splash_base_url: program.splash_api_base.clone(),
        test_controls: request.test_controls.clone(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DaemonCycleSummary {
    pub duration_ms: u64,
    pub enabled_markets: usize,
    pub markets_attempted: usize,
    pub markets_processed: u64,
    pub runtime_market_slot_count: u64,
    pub stale_open_sweep_checked_offer_count: u64,
    pub stale_open_sweep_requeue_market_ids: Vec<String>,
    pub stale_open_sweep_requeue_count: usize,
    pub stale_open_sweep_truncated: bool,
    pub immediate_requeue_market_ids: Vec<String>,
    pub immediate_requeue_count: usize,
    pub error_count: u64,
    pub strategy_planned_total: u64,
    pub strategy_executed_total: u64,
    pub cancel_triggered_count: u64,
    pub cancel_planned_total: u64,
    pub cancel_executed_total: u64,
    pub consumed_immediate_requeues: Vec<String>,
}

pub fn build_cycle_summary(
    plan: &CyclePlan,
    metrics: &MarketDispatchMetrics,
    preamble_error_count: u64,
    duration_ms: u64,
) -> DaemonCycleSummary {
    let deduped_requeue_market_ids =
        dedupe_sorted_market_ids(&metrics.immediate_requeue_market_ids);
    DaemonCycleSummary {
        duration_ms,
        enabled_markets: plan.enabled_market_ids.len(),
        markets_attempted: plan.selected_market_ids.len(),
        markets_processed: metrics.markets_processed,
        runtime_market_slot_count: plan.configured_market_slot_count,
        stale_open_sweep_checked_offer_count: plan.stale_open_sweep.checked_offer_count as u64,
        stale_open_sweep_requeue_market_ids: plan.stale_open_sweep.requeue_market_ids.clone(),
        stale_open_sweep_requeue_count: plan.stale_open_sweep.requeue_market_ids.len(),
        stale_open_sweep_truncated: plan.stale_open_sweep.truncated,
        immediate_requeue_market_ids: deduped_requeue_market_ids.clone(),
        immediate_requeue_count: deduped_requeue_market_ids.len(),
        error_count: preamble_error_count + metrics.cycle_error_count,
        strategy_planned_total: metrics.strategy_planned_total,
        strategy_executed_total: metrics.strategy_executed_total,
        cancel_triggered_count: metrics.cancel_triggered_count,
        cancel_planned_total: metrics.cancel_planned_total,
        cancel_executed_total: metrics.cancel_executed_total,
        consumed_immediate_requeues: plan.consumed_immediate_requeues.clone(),
    }
}

pub fn cycle_started_instant() -> Instant {
    Instant::now()
}

pub fn elapsed_ms(started: Instant) -> u64 {
    millis_to_u64(started.elapsed().as_millis())
}

pub fn compute_cycle_exit_code(plan: &CyclePlan, metrics: &MarketDispatchMetrics) -> i32 {
    let attempted = plan.selected_market_ids.len();
    if attempted > 0 && metrics.markets_processed == 0 {
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_cycle_exit_code_non_zero_when_all_markets_fail() {
        let plan = CyclePlan {
            enabled_market_ids: vec!["m1".to_string()],
            selected_market_ids: vec!["m1".to_string()],
            consumed_immediate_requeues: Vec::new(),
            dispatch_state: DaemonDispatchState::default(),
            stale_open_sweep: StaleSweepProgress {
                checked_offer_count: 0,
                requeue_market_ids: Vec::new(),
                hits: Vec::new(),
                truncated: false,
            },
            configured_market_slot_count: 1,
            runtime_dry_run: false,
            db_path: PathBuf::from("/tmp/db.sqlite"),
            previous_xch_price_usd: None,
            dexie_base_url: String::new(),
            splash_base_url: String::new(),
            test_controls: DaemonCycleTestControls::default(),
        };
        let metrics = MarketDispatchMetrics {
            markets_processed: 0,
            cycle_error_count: 1,
            ..MarketDispatchMetrics::default()
        };
        assert_eq!(compute_cycle_exit_code(&plan, &metrics), 1);
    }

    #[test]
    fn resolve_state_db_path_prefers_explicit_override() {
        use crate::storage::state_db_path_for_home;

        let home = PathBuf::from("/tmp/gf");
        assert_eq!(
            resolve_state_db_path(&home, Some("/tmp/custom.sqlite")),
            PathBuf::from("/tmp/custom.sqlite")
        );
        assert_eq!(
            resolve_state_db_path(&home, None),
            state_db_path_for_home(&home)
        );
    }

    #[test]
    fn test_controls_default_allowed_without_env_gate() {
        std::env::set_var("GREENFLOOR_DAEMON_TEST_CONTROLS", "0");
        let controls = DaemonCycleTestControls::default();
        assert!(controls.ensure_allowed().is_ok());
    }

    #[test]
    fn test_controls_non_default_rejected_without_env_gate() {
        std::env::set_var("GREENFLOOR_DAEMON_TEST_CONTROLS", "0");
        let controls = DaemonCycleTestControls {
            skip_strategy_execution: true,
            force_market_error_for: None,
        };
        let err = controls.ensure_allowed().expect_err("gate");
        assert!(err
            .to_string()
            .contains("GREENFLOOR_DAEMON_TEST_CONTROLS=1"));
    }

    #[test]
    fn test_controls_non_default_allowed_when_env_gate_set() {
        std::env::set_var("GREENFLOOR_DAEMON_TEST_CONTROLS", "1");
        let controls = DaemonCycleTestControls {
            skip_strategy_execution: true,
            force_market_error_for: Some("m1".to_string()),
        };
        assert!(controls.ensure_allowed().is_ok());
        std::env::remove_var("GREENFLOOR_DAEMON_TEST_CONTROLS");
    }
}
