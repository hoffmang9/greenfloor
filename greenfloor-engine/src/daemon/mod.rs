//! Daemon cycle orchestration (native Rust). Reconcile, inventory/strategy/coin_ops planning, and cancel run in Rust.

mod cancel_phase;
mod coin_ops_execution;
mod coin_ops_phase;
mod coinset_tx;
mod coinset_ws;
mod cycle_entry;
mod inventory_phase;
mod lock;
mod logging;
mod market_cycle;
mod market_dispatch;
mod market_gate;
mod market_phases;
mod preamble;
mod program_runtime;
mod reconcile_phase;
mod run_once;
mod stale_sweep;
mod strategy_phase;
mod strategy_support;
mod watchlist;

pub use cancel_phase::{run_market_cancel_phase, CancelPhaseMetrics};
pub use cycle_entry::{run_daemon_cycle_once, DaemonCycleOnceResponse};
pub use lock::DaemonInstanceLock;
pub use logging::{default_log_level, initialize_daemon_file_logging, warn_if_daemon_log_level_auto_healed};
pub use market_dispatch::{
    aggregate_market_dispatch_metrics, dexie_client, program_network, record_market_worker_error,
    selected_markets, IoPhaseMetrics, MarketDispatchContext, SingleMarketCycleOutput,
};
pub use program_runtime::{
    default_testnet_markets_path, load_daemon_program_runtime, resolve_testnet_markets_path,
    use_websocket_capture_for_once, DaemonProgramRuntime,
};
pub use reconcile_phase::{run_market_reconcile_phase, ReconcilePhaseMetrics, ReconcilePhaseResult};
pub use run_once::{
    build_cycle_summary, build_cycle_plan, compute_cycle_exit_code, resolve_state_db_path,
    cycle_started_instant, elapsed_ms, CyclePlan, DaemonDispatchState, DaemonRunOnceRequest,
    MarketDispatchMetrics,
};
