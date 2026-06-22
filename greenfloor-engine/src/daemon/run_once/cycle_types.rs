use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::cycle::StaleSweepProgress;

use super::request::{DaemonCycleTestControls, DaemonDispatchState};

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
