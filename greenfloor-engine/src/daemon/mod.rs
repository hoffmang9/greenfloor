//! Daemon cycle orchestration (native Rust). Reconcile, inventory/strategy/coin_ops planning, and cancel run in Rust.

mod cancel_phase;
mod coin_ops_execution;
mod coin_ops_phase;
mod coinset_spendable;
mod coinset_tx;
mod coinset_ws;
mod cycle_entry;
mod disabled_markets;
mod inventory_phase;
mod lock;
mod logging;
mod market_context;
mod market_cycle;
mod market_dispatch;
mod market_gate;
mod offer_dispatch;
mod preamble;
mod program_runtime;
mod reconcile_augment;
mod reconcile_phase;
mod run_once;
mod stale_sweep;
mod strategy_phase;
mod strategy_support;
pub mod watchlist;

pub use coinset_ws::{
    resolve_coinset_ws_url, start_coinset_websocket_loop, CoinsetWebsocketLoopHandle,
};
pub use watchlist::{
    active_offer_counts_by_size, active_offer_counts_by_size_and_side,
    active_offer_counts_by_size_and_side_detail, active_offer_counts_by_size_detail,
    watchlist_offer_ids,
};

pub use cancel_phase::run_market_cancel_phase;
pub use cycle_entry::{run_daemon_cycle_once, DaemonCycleOnceResponse};
pub use lock::DaemonInstanceLock;
pub use logging::{
    default_log_level, initialize_daemon_file_logging, warn_if_daemon_log_level_auto_healed,
};
pub use market_context::{
    load_cycle_resources, DaemonCycleResources, MarketCycleContext, MarketDispatchContext,
};
pub use market_dispatch::{
    aggregate_market_dispatch_metrics, record_market_worker_error, SingleMarketCycleOutput,
};
pub use program_runtime::{
    default_testnet_markets_path, load_daemon_program_runtime, resolve_testnet_markets_path,
    use_websocket_capture_for_once, websocket_capture_enabled, DaemonProgramRuntime,
};
pub use reconcile_phase::{
    run_market_reconcile_phase, ReconcilePhaseMetrics, ReconcilePhaseResult,
};
pub use run_once::{
    build_cycle_plan, build_cycle_summary, compute_cycle_exit_code, cycle_started_instant,
    elapsed_ms, resolve_state_db_path, CyclePlan, DaemonCycleSummary, DaemonCycleTestControls,
    DaemonDispatchState, DaemonRunOnceRequest, MarketDispatchMetrics,
};
