use std::path::PathBuf;

use clap::Args;
use serde_json::Value;

use crate::config::load_program_config;
use crate::error::{SignerError, SignerResult};
use crate::storage::resolve_state_db_path;

use super::cycle_entry::{run_daemon_cycle_once, DaemonCycleOnceResponse};
use super::daemon_loop::{run_daemon_loop, DaemonLoopRequest};
use super::lock::DaemonInstanceLock;
use super::logging::{initialize_daemon_file_logging, warn_if_daemon_log_level_auto_healed};
use super::offer_lifecycle_cli::{offers_cancel_cli, offers_status_cli, OffersCancelCliResult};
use super::program_runtime::{load_daemon_program_runtime, use_websocket_capture_for_once};
use super::reconcile_batch::reconcile_offers_cli;
use super::run_once::{DaemonDispatchState, DaemonRunOnceRequestBody};
use super::watchlist::cache::CoinWatchlistCache;

fn print_json(value: &impl serde::Serialize) -> SignerResult<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|err| {
            SignerError::Other(format!("failed to encode json output: {err}"))
        })?
    );
    Ok(())
}

fn optional_trimmed(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

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

#[derive(Debug, Args)]
pub struct OffersReconcileCliArgs {
    #[arg(long, default_value = "config/program.yaml")]
    pub program_config: PathBuf,
    #[arg(long, default_value = "")]
    pub state_db: String,
    #[arg(long, default_value = "")]
    pub market_id: String,
    #[arg(long, default_value_t = 200)]
    pub limit: usize,
    #[arg(long)]
    pub venue: Option<String>,
}

#[derive(Debug, Args)]
pub struct OffersStatusCliArgs {
    #[arg(long, default_value = "config/program.yaml")]
    pub program_config: PathBuf,
    #[arg(long, default_value = "")]
    pub state_db: String,
    #[arg(long, default_value = "")]
    pub market_id: String,
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
    #[arg(long, default_value_t = 30)]
    pub events_limit: usize,
}

#[derive(Debug, Args)]
pub struct OffersCancelCliArgs {
    #[arg(long, default_value = "config/program.yaml")]
    pub program_config: PathBuf,
    #[arg(long, action = clap::ArgAction::Append)]
    pub offer_id: Vec<String>,
    #[arg(long)]
    pub cancel_open: bool,
    #[arg(long)]
    pub venue: Option<String>,
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
    warn_if_daemon_log_level_auto_healed(
        runtime.app_log_level_was_missing,
        &args.program_config,
    );

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

pub async fn run_offers_reconcile_command(args: OffersReconcileCliArgs) -> SignerResult<i32> {
    let program = load_program_config(&args.program_config)?;
    let db_path = resolve_state_db_path(&program.home_dir, optional_trimmed(&args.state_db).as_deref());
    let venue = args
        .venue
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(program.offer_publish_venue.as_str())
        .to_ascii_lowercase();
    let payload = reconcile_offers_cli(
        &db_path,
        &program.dexie_api_base,
        &venue,
        optional_trimmed(&args.market_id).as_deref(),
        args.limit,
    )
    .await?;
    print_json(&payload)?;
    Ok(0)
}

pub fn run_offers_status_command(args: OffersStatusCliArgs) -> SignerResult<i32> {
    let program = load_program_config(&args.program_config)?;
    let db_path = resolve_state_db_path(&program.home_dir, optional_trimmed(&args.state_db).as_deref());
    let payload = offers_status_cli(
        &db_path,
        optional_trimmed(&args.market_id).as_deref(),
        args.limit,
        args.events_limit,
    )?;
    print_json(&payload)?;
    Ok(0)
}

pub async fn run_offers_cancel_command(args: OffersCancelCliArgs) -> SignerResult<i32> {
    let program = load_program_config(&args.program_config)?;
    let db_path = resolve_state_db_path(&program.home_dir, None);
    let venue = args
        .venue
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(program.offer_publish_venue.as_str())
        .to_ascii_lowercase();
    let payload: OffersCancelCliResult = offers_cancel_cli(
        &db_path,
        &program.dexie_api_base,
        &venue,
        &args.offer_id,
        args.cancel_open,
    )
    .await?;
    let exit_code = if payload.failed_count == 0 { 0 } else { 2 };
    print_json(&payload)?;
    Ok(exit_code)
}

pub async fn run_daemon_cycle_once_from_json(value: Value) -> SignerResult<DaemonCycleOnceResponse> {
    let request = super::run_once::DaemonRunOnceRequest::from_json_value(value, CoinWatchlistCache::new())?;
    run_daemon_cycle_once(&request).await
}

pub async fn run_daemon_loop_from_json(value: Value) -> SignerResult<i32> {
    let request: DaemonLoopRequest =
        serde_json::from_value(value).map_err(|err| SignerError::Other(err.to_string()))?;
    run_daemon_loop(request).await
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
