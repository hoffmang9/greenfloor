use std::path::{Path, PathBuf};

use crate::config::load_program_config;
use crate::error::SignerResult;

#[derive(Debug, Clone)]
pub struct DaemonProgramRuntime {
    pub home_dir: PathBuf,
    pub app_log_level: String,
    pub app_log_level_was_missing: bool,
    pub runtime_loop_interval_seconds: u64,
    pub tx_block_trigger_mode: String,
}

/// Load daemon program runtime.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn load_daemon_program_runtime(program_path: &Path) -> SignerResult<DaemonProgramRuntime> {
    let program = load_program_config(program_path)?;
    Ok(DaemonProgramRuntime {
        home_dir: program.home_dir,
        app_log_level: program.app_log_level,
        app_log_level_was_missing: program.app_log_level_was_missing,
        runtime_loop_interval_seconds: program.runtime_loop_interval_seconds,
        tx_block_trigger_mode: program.tx_block_trigger_mode,
    })
}

#[must_use]
pub fn use_websocket_capture_for_once(runtime: &DaemonProgramRuntime) -> bool {
    websocket_capture_enabled(&runtime.tx_block_trigger_mode)
}

#[must_use]
pub fn websocket_capture_enabled(tx_block_trigger_mode: &str) -> bool {
    tx_block_trigger_mode
        .trim()
        .eq_ignore_ascii_case("websocket")
}

#[must_use]
pub fn resolve_testnet_markets_path(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = PathBuf::from(trimmed);
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

#[must_use]
pub fn default_testnet_markets_path() -> Option<PathBuf> {
    let candidate = PathBuf::from("~/.greenfloor/config/testnet-markets.yaml");
    let expanded = crate::paths::expand_home(candidate);
    if expanded.exists() {
        Some(expanded)
    } else {
        None
    }
}
