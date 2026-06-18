use std::path::PathBuf;

use clap::Args;
use serde_json::Value;

use crate::cli_util::optional_trimmed;
use crate::error::{SignerError, SignerResult};

use super::cycle_entry::{run_daemon_cycle_once, DaemonCycleOnceResponse};
use super::daemon_loop::{run_daemon_loop, DaemonLoopRequest};
use super::lock::DaemonInstanceLock;
use super::logging::{initialize_daemon_file_logging, warn_if_daemon_log_level_auto_healed};
use super::program_runtime::{load_daemon_program_runtime, use_websocket_capture_for_once};
use super::run_once::{DaemonDispatchState, DaemonRunOnceRequestBody};
use super::watchlist::cache::CoinWatchlistCache;

fn parse_key_ids(raw: &str) -> Option<Vec<String>> {
    let ids: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect();
    if ids.is_empty() {
        None
    } else {
        Some(ids)
    }
}

fn resolve_testnet_markets_path(raw: &str) -> Option<PathBuf> {
    super::program_runtime::resolve_testnet_markets_path(raw)
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

pub async fn run_daemon_command(args: DaemonCliArgs) -> SignerResult<i32> {
    let state_dir = args.state_dir.expanduser();
    let mode = if args.once { "once" } else { "loop" };
    let lock = match DaemonInstanceLock::acquire(&state_dir, mode) {
        Ok(lock) => lock,
        Err(SignerError::DaemonAlreadyRunning { .. }) => return Ok(3),
        Err(err) => return Err(err),
    };
    let _guard = lock;

    let runtime = load_daemon_program_runtime(&args.program_config)?;
    initialize_daemon_file_logging(&runtime.home_dir, &runtime.app_log_level)?;
    warn_if_daemon_log_level_auto_healed(runtime.app_log_level_was_missing, &args.program_config);

    let testnet_markets_path = resolve_testnet_markets_path(&args.testnet_markets_config);
    let allowed_key_ids = parse_key_ids(&args.key_ids).unwrap_or_default();
    let state_db_override = optional_trimmed(&args.state_db);

    if args.once {
        let use_websocket_capture = use_websocket_capture_for_once(&runtime);
        let body = DaemonRunOnceRequestBody {
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
            test_controls: Default::default(),
        };
        let request = body.into_engine(CoinWatchlistCache::new());
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
    run_daemon_loop(request).await
}

pub async fn run_daemon_cycle_once_from_json(
    value: Value,
) -> SignerResult<DaemonCycleOnceResponse> {
    let request =
        super::run_once::DaemonRunOnceRequest::from_json_value(value, CoinWatchlistCache::new())?;
    run_daemon_cycle_once(&request).await
}

pub async fn run_daemon_loop_from_json(value: Value) -> SignerResult<i32> {
    let request: DaemonLoopRequest =
        serde_json::from_value(value).map_err(|err| SignerError::Other(err.to_string()))?;
    run_daemon_loop(request).await
}

#[derive(Debug, Args)]
pub struct DaemonOnceJsonArgs {
    #[arg(long)]
    pub request_json: PathBuf,
    #[arg(long)]
    pub json: bool,
}

pub async fn run_daemon_once_from_request_json(args: DaemonOnceJsonArgs) -> SignerResult<i32> {
    let raw = std::fs::read_to_string(&args.request_json)
        .map_err(|err| SignerError::Other(format!("read request json: {err}")))?;
    let value: Value = serde_json::from_str(&raw)
        .map_err(|err| SignerError::Other(format!("parse request json: {err}")))?;
    let body: DaemonRunOnceRequestBody = serde_json::from_value(value.clone())
        .map_err(|err| SignerError::Other(format!("parse daemon once request: {err}")))?;
    body.test_controls.ensure_allowed()?;
    let response = run_daemon_cycle_once_from_json(value).await?;
    if args.json {
        let encoded =
            serde_json::to_value(&response).map_err(|err| SignerError::Other(err.to_string()))?;
        crate::cli_util::print_json_value(&encoded, true)?;
    }
    Ok(response.exit_code)
}

trait PathExt {
    fn expanduser(self) -> PathBuf;
}

impl PathExt for PathBuf {
    fn expanduser(self) -> PathBuf {
        let raw = self.to_string_lossy();
        if raw == "~" {
            if let Ok(home) = std::env::var("HOME") {
                return PathBuf::from(home);
            }
        }
        if let Some(stripped) = raw.strip_prefix("~/") {
            if let Ok(home) = std::env::var("HOME") {
                return PathBuf::from(home).join(stripped);
            }
        }
        self
    }
}
