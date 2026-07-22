use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::load_program_config;
use crate::error::SignerResult;
use crate::storage::resolve_state_db_path;

use super::coinset_ws::{start_coinset_websocket_loop, CoinsetWsShared};
use super::cycle_entry::run_daemon_cycle_once;
use super::logging::{sync_daemon_file_logging, warn_if_log_level_auto_healed};
use super::program_runtime::{load_daemon_program_runtime, DaemonProgramRuntime};
use super::reload::handle_reload_marker_if_present;
use super::run_once::{DaemonCycleTestControls, DaemonDispatchState, DaemonRunOnceRequest};

#[cfg(test)]
use super::loop_harness::DaemonLoopTestHarness;

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

fn loop_cycle_test_controls(
    #[cfg(test)] harness: Option<&DaemonLoopTestHarness>,
) -> DaemonCycleTestControls {
    #[cfg(test)]
    if let Some(harness) = harness {
        return harness.cycle_test_controls.clone();
    }
    DaemonCycleTestControls::default()
}

fn loop_sleep_after_cycle(
    runtime: &DaemonProgramRuntime,
    #[cfg(test)] harness: Option<&DaemonLoopTestHarness>,
) -> Duration {
    #[cfg(test)]
    if let Some(harness) = harness {
        return harness.cycle_sleep;
    }
    Duration::from_secs(runtime.runtime_loop_interval_seconds.max(1))
}

fn loop_should_continue(
    cycles_completed: usize,
    #[cfg(test)] harness: Option<&DaemonLoopTestHarness>,
) -> bool {
    #[cfg(test)]
    if let Some(harness) = harness {
        return cycles_completed < harness.max_cycles;
    }
    let _ = cycles_completed;
    true
}

async fn run_one_loop_cycle(
    request: &DaemonLoopRequest,
    dispatch_state: &mut DaemonDispatchState,
    coinset: Arc<CoinsetWsShared>,
    test_controls: DaemonCycleTestControls,
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
        test_controls,
        coinset,
    };
    let response = run_daemon_cycle_once(&once_request).await?;
    *dispatch_state = response.dispatch_state;
    Ok(response.exit_code)
}

async fn run_daemon_loop_inner(
    request: DaemonLoopRequest,
    #[cfg(test)] harness: Option<DaemonLoopTestHarness>,
) -> SignerResult<i32> {
    let runtime = load_daemon_program_runtime(&request.program_path)?;
    sync_daemon_file_logging(&runtime.home_dir, &runtime.app_log_level)?;
    warn_if_log_level_auto_healed(runtime.app_log_level_was_missing, &request.program_path);

    let program = load_program_config(&request.program_path)?;
    let db_path = resolve_state_db_path(&program.home_dir, request.state_db_override.as_deref());
    let coinset = CoinsetWsShared::from_markets_or_empty(
        &request.markets_path,
        request.testnet_markets_path.as_deref(),
    );
    #[cfg(test)]
    let harness_ref = harness.as_ref();
    // In-process harness never starts the background WS thread (avoids connect/DNS
    // teardown hangs). Production `run_daemon_loop` always starts it.
    #[cfg(test)]
    let _ws_handle = if harness_ref.is_some() {
        None
    } else {
        Some(start_coinset_websocket_loop(
            db_path,
            program,
            request.coinset_base_url.clone(),
            Arc::clone(&coinset),
        ))
    };
    #[cfg(not(test))]
    let _ws_handle = start_coinset_websocket_loop(
        db_path,
        program,
        request.coinset_base_url.clone(),
        Arc::clone(&coinset),
    );

    let mut dispatch_state = DaemonDispatchState::default();
    let mut cycles_completed = 0usize;

    loop {
        let runtime = load_daemon_program_runtime(&request.program_path)?;
        sync_daemon_file_logging(&runtime.home_dir, &runtime.app_log_level)?;

        let exit_code = run_one_loop_cycle(
            &request,
            &mut dispatch_state,
            Arc::clone(&coinset),
            loop_cycle_test_controls(
                #[cfg(test)]
                harness_ref,
            ),
        )
        .await?;

        handle_reload_marker_if_present(
            &request.state_dir,
            &resolve_state_db_path(&runtime.home_dir, request.state_db_override.as_deref()),
            &coinset,
            &request.markets_path,
            request.testnet_markets_path.as_deref(),
        );

        cycles_completed += 1;
        if !loop_should_continue(
            cycles_completed,
            #[cfg(test)]
            harness_ref,
        ) {
            return Ok(exit_code);
        }

        tokio::time::sleep(loop_sleep_after_cycle(
            &runtime,
            #[cfg(test)]
            harness_ref,
        ))
        .await;
    }
}

/// Run the daemon loop until stopped (or harness max cycles in tests).
///
/// # Errors
///
/// Returns an error if a cycle fails fatally.
pub async fn run_daemon_loop(request: DaemonLoopRequest) -> SignerResult<i32> {
    Box::pin(run_daemon_loop_inner(
        request,
        #[cfg(test)]
        None,
    ))
    .await
}

#[cfg(test)]
pub(crate) async fn run_daemon_loop_with_harness(
    request: DaemonLoopRequest,
    harness: DaemonLoopTestHarness,
) -> SignerResult<i32> {
    Box::pin(run_daemon_loop_inner(request, Some(harness))).await
}
