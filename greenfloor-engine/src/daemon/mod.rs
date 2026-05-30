//! Daemon cycle orchestration (outer loop). Reconcile/cancel phases are native Rust; inventory/strategy/coin_ops use the Python IO bridge.

mod cancel_phase;
mod coinset_tx;
mod cycle_entry;
mod lock;
mod logging;
mod market_dispatch;
mod program_runtime;
mod python_bridge;
mod reconcile_phase;
mod run_once;
mod stale_sweep;

pub use cancel_phase::{run_market_cancel_phase, CancelPhaseMetrics};
pub use cycle_entry::{run_daemon_cycle_once, DaemonCycleOnceResponse};
pub use lock::DaemonInstanceLock;
pub use logging::{default_log_level, initialize_daemon_file_logging, warn_if_daemon_log_level_auto_healed};
pub use market_dispatch::{
    aggregate_market_dispatch_metrics, build_market_lookup, dexie_client, load_runtime_dry_run,
    open_store, program_network, reconcile_context_for_python, run_market_cancel_phase_for_market,
    run_market_reconcile_phase_for_market, selected_markets, IoPhaseMetrics, MarketDispatchContext,
    SingleMarketCycleOutput,
};
pub use program_runtime::{
    default_testnet_markets_path, load_daemon_program_runtime, resolve_testnet_markets_path,
    use_websocket_capture_for_once, DaemonProgramRuntime,
};
pub use python_bridge::{default_bridge, SubprocessPythonBridge};
pub use reconcile_phase::{run_market_reconcile_phase, ReconcilePhaseMetrics, ReconcilePhaseResult};
pub use run_once::{
    build_cycle_summary, build_cycle_plan, resolve_state_db_path, cycle_started_instant, elapsed_ms,
    CyclePlan, DaemonDispatchState, DaemonRunOnceRequest, MarketDispatchMetrics,
};
