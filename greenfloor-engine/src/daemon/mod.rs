//! Daemon cycle orchestration (native Rust). Reconcile, `inventory/strategy/coin_ops` planning, and cancel run in Rust.

mod cancel_phase;
mod cli;
mod coin_ops_execution;
mod coin_ops_phase;
mod coinset_spendable;
mod coinset_tx;
mod coinset_ws;
mod cycle_entry;
mod cycle_paths;
mod cycle_store;
mod daemon_loop;
mod disabled_markets;
#[cfg(test)]
mod dispatch_test_controls;
mod inventory_freshness;
mod inventory_phase;
mod lock;
mod logging;
#[cfg(test)]
mod loop_harness;
mod market_context;
mod market_cycle;
mod market_dispatch;
mod market_gate;
mod markets;
mod offer_dispatch;
mod preamble;
mod program_runtime;
mod reconcile_augment;
mod reconcile_market_cycle;
mod reload;
mod run_once;
mod stale_sweep;
mod strategy_phase;
mod strategy_support;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
pub use coin_ops_phase::harness::CoinOpsPhaseHarness;
pub mod watchlist;

pub use crate::coin_ops::execution::CoinOpExecContext;
pub use crate::offer::lifecycle::{
    cancel_offers_on_chain, reconcile_offers_batch, reconcile_offers_cli, CancelOfferTarget,
    ReconcileBatchItem, ReconcileBatchResult, ReconcileCliResult,
};
pub use cancel_phase::run_market_cancel_phase;
pub use cli::{
    run_daemon_command, run_daemon_cycle_once_from_json, run_daemon_loop_from_json,
    run_daemon_once_from_request_json, DaemonCliArgs, DaemonOnceJsonArgs,
};
pub use coin_ops_execution::{
    execute_managed_coin_op_plans, persist_coin_op_execution, watched_coin_ids_from_open_offers,
    CoinOpExecutionResult,
};
pub use coinset_tx::build_dexie_size_by_offer_id;
pub use coinset_ws::{
    resolve_coinset_ws_url_with_p2s, stable_inventory_p2s_from_markets,
    start_coinset_websocket_loop, CoinsetWebsocketLoopHandle,
};
pub use cycle_entry::{run_daemon_cycle_once, DaemonCycleOnceResponse};
pub use cycle_paths::DaemonCyclePaths;
pub use daemon_loop::{run_daemon_loop, DaemonLoopRequest};
pub use inventory_freshness::{InventoryFreshnessCache, INVENTORY_MAX_STALENESS};
pub use inventory_phase::{assert_inventory_asset_resolution_matches_config, run_inventory_phase};
pub use lock::DaemonInstanceLock;
pub use logging::{default_log_level, sync_daemon_file_logging, warn_if_log_level_auto_healed};
pub use market_context::{
    load_cycle_resources, DaemonCycleResources, MarketCycleContext, MarketDispatchContext,
};
pub use market_dispatch::{
    aggregate_market_dispatch_metrics, record_market_worker_error, SingleMarketCycleOutput,
};
pub use markets::enabled_market_ids;
pub(crate) use offer_dispatch::OfferDispatchOutput;
pub use program_runtime::{
    load_daemon_program_runtime, use_websocket_capture_for_once, websocket_capture_enabled,
    DaemonProgramRuntime,
};
pub use reload::{record_config_reloaded, reload_marker_present, remove_reload_marker};
pub use run_once::{
    build_cycle_plan, build_cycle_summary, compute_cycle_exit_code, cycle_started_instant,
    elapsed_ms, CyclePlan, DaemonCycleSummary, DaemonCycleTestControls, DaemonDispatchState,
    DaemonRunOnceRequest, MarketDispatchMetrics,
};
pub use watchlist::{
    active_offer_counts_by_size, active_offer_counts_by_size_and_side,
    active_offer_counts_by_size_and_side_detail, active_offer_counts_by_size_detail,
    watchlist_offer_ids, RESEED_MEMPOOL_MAX_AGE_SECONDS,
};
