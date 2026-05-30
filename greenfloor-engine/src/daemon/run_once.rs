use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::{load_markets_config_with_overlay, load_program_config};
use crate::cycle::{
    dedupe_sorted_market_ids, enqueue_immediate_requeue, select_market_batch,
    should_use_market_slot_dispatch, StaleSweepProgress,
};
use crate::error::{SignerError, SignerResult};
use crate::storage::{state_db_path_for_home, SqliteStore};

use super::stale_sweep::detect_stale_open_offers_for_requeue;
use crate::adapters::DexieClient;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DaemonDispatchState {
    pub cursor: usize,
    pub immediate_requeue_ids: Vec<String>,
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
    pub dispatch_state: DaemonDispatchState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CyclePlan {
    pub enabled_market_ids: Vec<String>,
    pub selected_market_ids: Vec<String>,
    pub consumed_immediate_requeues: Vec<String>,
    pub dispatch_state: DaemonDispatchState,
    pub stale_open_sweep: StaleSweepProgress,
    pub configured_market_slot_count: u64,
    pub parallel_markets_enabled: bool,
    pub db_path: PathBuf,
    pub previous_xch_price_usd: Option<f64>,
    pub dexie_base_url: String,
    pub splash_base_url: String,
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

pub fn resolve_state_db_path(
    home_dir: &Path,
    explicit_db_path: Option<&str>,
) -> PathBuf {
    if let Some(path) = explicit_db_path.map(str::trim).filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    state_db_path_for_home(home_dir)
}

pub async fn build_cycle_plan(request: &DaemonRunOnceRequest) -> SignerResult<CyclePlan> {
    let program = load_program_config(&request.program_path)?;
    let markets = load_markets_config_with_overlay(
        &request.markets_path,
        request.testnet_markets_path.as_deref(),
    )?;
    let db_path = resolve_state_db_path(
        &program.home_dir,
        request.state_db_override.as_deref(),
    );
    let store = SqliteStore::open(&db_path)?;
    let previous_xch_price_usd = store.get_latest_xch_price_snapshot()?;

    let mut enabled_market_ids: Vec<String> = Vec::new();
    let mut enabled_set: HashSet<String> = HashSet::new();
    for market in &markets.markets {
        if !market.enabled {
            continue;
        }
        let market_id = market.market_id.trim();
        if market_id.is_empty() || enabled_set.contains(market_id) {
            continue;
        }
        enabled_set.insert(market_id.to_string());
        enabled_market_ids.push(market_id.to_string());
    }

    let dexie = DexieClient::new(program.dexie_api_base.clone());
    let stale_open_sweep = if enabled_market_ids.is_empty() {
        StaleSweepProgress {
            checked_offer_count: 0,
            requeue_market_ids: Vec::new(),
            hits: Vec::new(),
            truncated: false,
        }
    } else {
        detect_stale_open_offers_for_requeue(&store, &dexie, &enabled_market_ids).await?
    };

    let runtime_fields = load_daemon_runtime_fields(&request.program_path)?;
    let mut dispatch_state = request.dispatch_state.clone();
    for market_id in &stale_open_sweep.requeue_market_ids {
        dispatch_state.immediate_requeue_ids =
            enqueue_immediate_requeue(&dispatch_state.immediate_requeue_ids, market_id);
    }

    let configured_market_slot_count = runtime_fields.runtime_market_slot_count;
    let (selected_market_ids, consumed_immediate_requeues) =
        if should_use_market_slot_dispatch(enabled_market_ids.len(), configured_market_slot_count as usize)
        {
            let selection = select_market_batch(
                &enabled_market_ids,
                configured_market_slot_count as usize,
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
        configured_market_slot_count,
        parallel_markets_enabled: runtime_fields.runtime_parallel_markets,
        db_path,
        previous_xch_price_usd,
        dexie_base_url: program.dexie_api_base,
        splash_base_url: program.splash_api_base,
    })
}

pub fn build_cycle_summary(
    plan: &CyclePlan,
    metrics: &MarketDispatchMetrics,
    preamble_error_count: u64,
    duration_ms: u64,
) -> Value {
    let deduped_requeue_market_ids =
        dedupe_sorted_market_ids(&metrics.immediate_requeue_market_ids);
    json!({
        "duration_ms": duration_ms,
        "enabled_markets": plan.enabled_market_ids.len(),
        "markets_attempted": plan.selected_market_ids.len(),
        "markets_processed": metrics.markets_processed,
        "runtime_market_slot_count": plan.configured_market_slot_count,
        "stale_open_sweep_checked_offer_count": plan.stale_open_sweep.checked_offer_count,
        "stale_open_sweep_requeue_market_ids": plan.stale_open_sweep.requeue_market_ids,
        "stale_open_sweep_requeue_count": plan.stale_open_sweep.requeue_market_ids.len(),
        "stale_open_sweep_truncated": plan.stale_open_sweep.truncated,
        "immediate_requeue_market_ids": deduped_requeue_market_ids,
        "immediate_requeue_count": deduped_requeue_market_ids.len(),
        "error_count": preamble_error_count + metrics.cycle_error_count,
        "strategy_planned_total": metrics.strategy_planned_total,
        "strategy_executed_total": metrics.strategy_executed_total,
        "cancel_triggered_count": metrics.cancel_triggered_count,
        "cancel_planned_total": metrics.cancel_planned_total,
        "cancel_executed_total": metrics.cancel_executed_total,
        "consumed_immediate_requeues": plan.consumed_immediate_requeues,
    })
}

struct DaemonRuntimeFields {
    runtime_market_slot_count: u64,
    runtime_parallel_markets: bool,
}

fn load_daemon_runtime_fields(program_path: &Path) -> SignerResult<DaemonRuntimeFields> {
    let raw = std::fs::read_to_string(program_path).map_err(|err| {
        SignerError::Other(format!("failed to read config {}: {err}", program_path.display()))
    })?;
    let parsed: serde_yaml::Value = serde_yaml::from_str(&raw).map_err(|err| {
        SignerError::Other(format!("failed to parse config {}: {err}", program_path.display()))
    })?;
    let runtime = parsed.get("runtime");
    let slot_count = runtime
        .and_then(|value| value.get("market_slot_count"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let parallel = runtime
        .and_then(|value| value.get("parallel_markets"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    Ok(DaemonRuntimeFields {
        runtime_market_slot_count: slot_count,
        runtime_parallel_markets: parallel,
    })
}

pub fn cycle_started_instant() -> Instant {
    Instant::now()
}

pub fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_state_db_path_prefers_explicit_override() {
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
}
