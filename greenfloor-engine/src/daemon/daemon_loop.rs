use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::load_program_config;
use crate::error::SignerResult;
use crate::storage::resolve_state_db_path;

use super::coinset_ws::start_coinset_websocket_loop;
use super::cycle_entry::run_daemon_cycle_once;
use super::logging::{sync_daemon_file_logging, warn_if_log_level_auto_healed};
use super::program_runtime::load_daemon_program_runtime;
use super::reload::handle_reload_marker_if_present;
use super::run_once::{DaemonCycleTestControls, DaemonDispatchState, DaemonRunOnceRequest};
use super::watchlist::cache::CoinWatchlistCache;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonLoopRequest {
    pub program_path: PathBuf,
    pub markets_path: PathBuf,
    pub testnet_markets_path: Option<PathBuf>,
    pub state_db_override: Option<String>,
    pub coinset_base_url: String,
    pub state_dir: PathBuf,
    pub allowed_key_ids: Vec<String>,
}

async fn run_one_loop_cycle(
    request: &DaemonLoopRequest,
    dispatch_state: &mut DaemonDispatchState,
    coin_watchlist: Arc<CoinWatchlistCache>,
) -> SignerResult<i32> {
    let once_request = DaemonRunOnceRequest {
        program_path: request.program_path.clone(),
        markets_path: request.markets_path.clone(),
        testnet_markets_path: request.testnet_markets_path.clone(),
        state_db_override: request.state_db_override.clone(),
        coinset_base_url: request.coinset_base_url.clone(),
        state_dir: request.state_dir.clone(),
        poll_coinset_mempool: false,
        use_websocket_capture: false,
        allowed_key_ids: request.allowed_key_ids.clone(),
        dispatch_state: dispatch_state.clone(),
        test_controls: DaemonCycleTestControls::default(),
        coin_watchlist,
    };
    let response = run_daemon_cycle_once(&once_request).await?;
    *dispatch_state = response.dispatch_state;
    Ok(response.exit_code)
}

/// Run daemon loop.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_daemon_loop(request: DaemonLoopRequest) -> SignerResult<i32> {
    let runtime = load_daemon_program_runtime(&request.program_path)?;
    sync_daemon_file_logging(&runtime.home_dir, &runtime.app_log_level)?;
    warn_if_log_level_auto_healed(runtime.app_log_level_was_missing, &request.program_path);

    let program = load_program_config(&request.program_path)?;
    let db_path = resolve_state_db_path(&program.home_dir, request.state_db_override.as_deref());
    let coin_watchlist = CoinWatchlistCache::new();
    let _ws_handle = start_coinset_websocket_loop(
        db_path,
        program,
        request.coinset_base_url.clone(),
        coin_watchlist.clone(),
    );

    let mut dispatch_state = DaemonDispatchState::default();

    loop {
        let runtime = load_daemon_program_runtime(&request.program_path)?;
        sync_daemon_file_logging(&runtime.home_dir, &runtime.app_log_level)?;

        let _exit_code =
            run_one_loop_cycle(&request, &mut dispatch_state, coin_watchlist.clone()).await?;

        handle_reload_marker_if_present(
            &request.state_dir,
            &resolve_state_db_path(&runtime.home_dir, request.state_db_override.as_deref()),
        );

        tokio::time::sleep(Duration::from_secs(
            runtime.runtime_loop_interval_seconds.max(1),
        ))
        .await;
    }
}
