use std::path::{Path, PathBuf};

use serde_json::Value;

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

pub fn load_daemon_program_runtime(program_path: &Path) -> SignerResult<DaemonProgramRuntime> {
    let program = load_program_config(program_path)?;
    let raw = std::fs::read_to_string(program_path).map_err(|err| {
        crate::error::SignerError::Other(format!(
            "failed to read config {}: {err}",
            program_path.display()
        ))
    })?;
    let parsed: Value = serde_yaml::from_str(&raw).map_err(|err| {
        crate::error::SignerError::Other(format!(
            "failed to parse config {}: {err}",
            program_path.display()
        ))
    })?;
    let runtime = parsed.get("runtime");
    let loop_interval = runtime
        .and_then(|value| value.get("loop_interval_seconds"))
        .and_then(Value::as_u64)
        .unwrap_or(30)
        .max(1);
    let trigger_mode = parsed
        .pointer("/chain_signals/tx_block_trigger/mode")
        .and_then(Value::as_str)
        .unwrap_or("websocket")
        .trim()
        .to_ascii_lowercase();
    Ok(DaemonProgramRuntime {
        home_dir: program.home_dir,
        app_log_level: program.app_log_level,
        app_log_level_was_missing: program.app_log_level_was_missing,
        runtime_loop_interval_seconds: loop_interval,
        tx_block_trigger_mode: trigger_mode,
    })
}

pub fn use_websocket_capture_for_once(runtime: &DaemonProgramRuntime) -> bool {
    runtime.tx_block_trigger_mode == "websocket"
}

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

pub fn default_testnet_markets_path() -> Option<PathBuf> {
    let candidate = PathBuf::from("~/.greenfloor/config/testnet-markets.yaml");
    let expanded = expand_home(&candidate.to_string_lossy());
    if expanded.exists() {
        Some(expanded)
    } else {
        None
    }
}

fn expand_home(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    }
    PathBuf::from(path)
}
