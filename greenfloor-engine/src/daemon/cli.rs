use std::path::PathBuf;
use std::sync::Arc;

use clap::Args;
use serde_json::Value;

use crate::cli_util::optional_trimmed;
use crate::error::{SignerError, SignerResult};
use crate::paths::{expand_home, resolve_testnet_markets_path, TestnetMarketsPathPolicy};

use super::cycle_entry::{run_daemon_cycle_once, DaemonCycleOnceResponse};
use super::daemon_loop::{run_daemon_loop, DaemonLoopRequest};
use super::lock::DaemonInstanceLock;
use super::logging::{sync_daemon_file_logging, warn_if_log_level_auto_healed};
use super::program_runtime::{load_daemon_program_runtime, use_websocket_capture_for_once};
use super::run_once::{DaemonCycleTestControls, DaemonDispatchState, DaemonRunOnceRequest};

fn parse_key_ids(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

#[derive(Debug, Args)]
pub struct DaemonCliArgs {
    #[arg(long, default_value = "config/program.yaml")]
    pub program_config: PathBuf,
    #[arg(long, default_value = "config/markets.yaml")]
    pub markets_config: PathBuf,
    #[arg(long, default_value = "")]
    pub testnet_markets_config: String,
    #[arg(long, default_value = "")]
    pub key_ids: String,
    #[arg(long)]
    pub once: bool,
    #[arg(long, default_value = "")]
    pub state_db: String,
    #[arg(long, default_value = "https://api.coinset.org")]
    pub coinset_base_url: String,
    #[arg(long, default_value = "~/.greenfloor/state")]
    pub state_dir: PathBuf,
}

/// Run daemon command.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_daemon_command(args: DaemonCliArgs) -> SignerResult<i32> {
    let state_dir = expand_home(&args.state_dir);
    let mode = if args.once { "once" } else { "loop" };
    let lock = match DaemonInstanceLock::acquire(&state_dir, mode) {
        Ok(lock) => lock,
        Err(SignerError::DaemonAlreadyRunning { .. }) => return Ok(3),
        Err(err) => return Err(err),
    };
    let _guard = lock;

    let runtime = load_daemon_program_runtime(&args.program_config)?;
    sync_daemon_file_logging(&runtime.home_dir, &runtime.app_log_level)?;
    warn_if_log_level_auto_healed(runtime.app_log_level_was_missing, &args.program_config);

    let testnet_markets_path = resolve_testnet_markets_path(
        &args.testnet_markets_config,
        TestnetMarketsPathPolicy::RequireExistingFile,
    );
    let allowed_key_ids = parse_key_ids(&args.key_ids);
    let state_db_override = optional_trimmed(&args.state_db);

    if args.once {
        let use_websocket_capture = use_websocket_capture_for_once(&runtime);
        let request = DaemonRunOnceRequest {
            program_path: args.program_config,
            markets_path: args.markets_config,
            testnet_markets_path,
            state_db_override,
            coinset_base_url: args.coinset_base_url,
            state_dir,
            poll_coinset_mempool: !use_websocket_capture,
            use_websocket_capture,
            allowed_key_ids,
            dispatch_state: DaemonDispatchState::default(),
            test_controls: DaemonCycleTestControls::default(),
            inventory_freshness: crate::daemon::InventoryFreshnessCache::new(),
            inventory_p2s: Arc::new(crate::daemon::coinset_ws::InventoryP2Index::default()),
        };
        let response = run_daemon_cycle_once(&request).await?;
        return Ok(response.exit_code);
    }

    let request = DaemonLoopRequest {
        program_path: args.program_config,
        markets_path: args.markets_config,
        testnet_markets_path,
        state_db_override,
        coinset_base_url: args.coinset_base_url,
        state_dir,
        allowed_key_ids,
    };
    Box::pin(run_daemon_loop(request)).await
}

/// Run daemon cycle once from json.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_daemon_cycle_once_from_json(
    value: Value,
) -> SignerResult<DaemonCycleOnceResponse> {
    let request = parse_daemon_run_once_request(value)?;
    run_daemon_cycle_once(&request).await
}

fn parse_daemon_run_once_request(value: Value) -> SignerResult<DaemonRunOnceRequest> {
    DaemonRunOnceRequest::from_json_value(value)
}

/// Run daemon loop from json.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_daemon_loop_from_json(value: Value) -> SignerResult<i32> {
    let request: DaemonLoopRequest =
        serde_json::from_value(value).map_err(|err| SignerError::Other(err.to_string()))?;
    Box::pin(run_daemon_loop(request)).await
}

#[derive(Debug, Args)]
pub struct DaemonOnceJsonArgs {
    #[arg(long)]
    pub request_json: PathBuf,
    #[arg(long)]
    pub json: bool,
}

/// Run daemon once from request json.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_daemon_once_from_request_json(args: DaemonOnceJsonArgs) -> SignerResult<i32> {
    let raw = std::fs::read_to_string(&args.request_json)
        .map_err(|err| SignerError::Other(format!("read request json: {err}")))?;
    let value: Value = serde_json::from_str(&raw)
        .map_err(|err| SignerError::Other(format!("parse request json: {err}")))?;
    let request = parse_daemon_run_once_request(value)?;
    request.test_controls.ensure_allowed()?;
    let response = run_daemon_cycle_once(&request).await?;
    if args.json {
        let encoded =
            serde_json::to_value(&response).map_err(|err| SignerError::Other(err.to_string()))?;
        crate::cli_util::print_json_value(&encoded, true)?;
    }
    Ok(response.exit_code)
}

#[cfg(test)]
mod tests {
    use super::{parse_daemon_run_once_request, parse_key_ids};
    use serde_json::json;

    #[test]
    fn parse_key_ids_splits_and_trims_csv_values() {
        assert_eq!(
            parse_key_ids(" key-a , ,key-b"),
            vec!["key-a".to_string(), "key-b".to_string()]
        );
        assert!(parse_key_ids(" , ").is_empty());
    }

    #[test]
    fn parse_daemon_run_once_request_reads_testnet_markets_path() {
        let value = json!({
            "program_path": "config/program.yaml",
            "markets_path": "config/markets.yaml",
            "testnet_markets_path": "/tmp/testnet-markets.yaml",
            "coinset_base_url": "https://api.coinset.org",
            "state_dir": "/tmp/state",
        });
        let request = parse_daemon_run_once_request(value).expect("parse request");
        assert_eq!(
            request.testnet_markets_path.as_deref(),
            Some(std::path::Path::new("/tmp/testnet-markets.yaml"))
        );
    }
}
