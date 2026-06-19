use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::reload::{self, Handle};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::error::{SignerError, SignerResult};

const SERVICE_NAME: &str = "daemon";
const DEFAULT_LOG_LEVEL: &str = "INFO";
const LOG_FILE: &str = "logs/debug.log";

struct DaemonLogState {
    home_dir: PathBuf,
    filter_handle: Handle<EnvFilter, tracing_subscriber::Registry>,
}

static LOG_STATE: OnceLock<Result<DaemonLogState, String>> = OnceLock::new();

fn normalize_log_level_name(log_level: &str) -> &'static str {
    match log_level.trim().to_ascii_uppercase().as_str() {
        "CRITICAL" => "CRITICAL",
        "ERROR" => "ERROR",
        "WARNING" => "WARNING",
        "INFO" => "INFO",
        "DEBUG" => "DEBUG",
        "NOTSET" => "NOTSET",
        _ => DEFAULT_LOG_LEVEL,
    }
}

fn env_filter_for_level(log_level: &str) -> EnvFilter {
    EnvFilter::new(normalize_log_level_name(log_level))
}

fn open_log_file(home_dir: &Path) -> Result<std::fs::File, String> {
    let log_path = home_dir.join(LOG_FILE);
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create daemon log dir {}: {err}",
                parent.display()
            )
        })?;
    }

    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|err| {
            format!(
                "failed to open daemon log file {}: {err}",
                log_path.display()
            )
        })
}

fn global_subscriber_conflict_message(init_err: impl std::fmt::Display) -> String {
    format!(
        "failed to install daemon file logging subscriber: {init_err} \
         (a global tracing subscriber is already active; daemon logging must initialize first)"
    )
}

fn install_daemon_file_logging(home_dir: &Path, log_level: &str) -> Result<DaemonLogState, String> {
    if tracing::dispatcher::has_been_set() {
        return Err(global_subscriber_conflict_message(
            "tracing dispatcher already set before daemon logging init",
        ));
    }

    let normalized = normalize_log_level_name(log_level);
    let log_path = home_dir.join(LOG_FILE);
    let file = open_log_file(home_dir)?;

    let (filter_layer, filter_handle) = reload::Layer::new(env_filter_for_level(log_level));
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file)
        .with_ansi(false)
        .with_target(true)
        .with_level(true)
        .with_span_events(FmtSpan::NONE);

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(file_layer)
        .try_init()
        .map_err(|err| {
            if tracing::dispatcher::has_been_set() {
                global_subscriber_conflict_message(err)
            } else {
                format!("failed to init daemon file logging subscriber: {err}")
            }
        })?;

    tracing::info!(
        service = SERVICE_NAME,
        log_path = %log_path.display(),
        log_level = normalized,
        "daemon file logging initialized"
    );

    Ok(DaemonLogState {
        home_dir: home_dir.to_path_buf(),
        filter_handle,
    })
}

fn reload_daemon_log_level(
    state: &DaemonLogState,
    home_dir: &Path,
    log_level: &str,
) -> Result<(), String> {
    if state.home_dir != home_dir {
        tracing::warn!(
            active_home = %state.home_dir.display(),
            requested_home = %home_dir.display(),
            "daemon log file path unchanged after home_dir change; restart process to relocate logs"
        );
    }

    let normalized = normalize_log_level_name(log_level);
    state
        .filter_handle
        .modify(|filter| *filter = env_filter_for_level(log_level))
        .map_err(|err| format!("failed to reload daemon log filter: {err}"))?;

    tracing::debug!(
        service = SERVICE_NAME,
        log_level = normalized,
        "daemon log level reloaded"
    );
    Ok(())
}

/// Initialize or refresh daemon file logging for the current process.
///
/// The first call installs the file subscriber under `{home_dir}/logs/debug.log`.
/// Later calls update the active `EnvFilter` when `log_level` changes (config reload /
/// `set-log-level`). The log file path stays fixed after first init — a changed `home_dir`
/// emits a warning until process restart.
///
/// # Errors
///
/// Returns an error if the first initialization attempt fails, including when another
/// global tracing subscriber was installed first.
pub fn sync_daemon_file_logging(home_dir: &Path, log_level: &str) -> SignerResult<()> {
    if let Some(state) = LOG_STATE.get() {
        return match state {
            Ok(active) => {
                reload_daemon_log_level(active, home_dir, log_level).map_err(SignerError::Other)
            }
            Err(message) => Err(SignerError::Other(message.clone())),
        };
    }

    let state = LOG_STATE.get_or_init(|| install_daemon_file_logging(home_dir, log_level));
    match state {
        Ok(_active) => Ok(()),
        Err(message) => Err(SignerError::Other(message.clone())),
    }
}

/// Initialize daemon file logging once per process.
///
/// Alias for [`sync_daemon_file_logging`] for call sites that only run at startup.
///
/// # Errors
///
/// Returns an error if initialization fails.
pub fn initialize_daemon_file_logging(home_dir: &Path, log_level: &str) -> SignerResult<()> {
    sync_daemon_file_logging(home_dir, log_level)
}

pub fn warn_if_daemon_log_level_auto_healed(
    log_level_was_missing: bool,
    program_config_path: &Path,
) {
    if log_level_was_missing {
        tracing::warn!(
            program_config = %program_config_path.display(),
            "program config missing app.log_level; defaulting to INFO"
        );
    }
}

#[must_use]
pub fn default_log_level() -> &'static str {
    DEFAULT_LOG_LEVEL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_log_level_defaults_invalid_to_info() {
        assert_eq!(normalize_log_level_name("debug"), "DEBUG");
        assert_eq!(normalize_log_level_name(""), DEFAULT_LOG_LEVEL);
        assert_eq!(normalize_log_level_name("verbose"), DEFAULT_LOG_LEVEL);
    }

    #[test]
    fn sync_daemon_file_logging_creates_log_file_and_reloads_level() {
        if tracing::dispatcher::has_been_set() {
            return;
        }

        let dir = tempfile::tempdir().expect("tempdir");
        sync_daemon_file_logging(dir.path(), "INFO").expect("init");
        let log_path = dir.path().join(LOG_FILE);
        assert!(log_path.is_file());

        sync_daemon_file_logging(dir.path(), "DEBUG").expect("reload");
        assert!(LOG_STATE.get().is_some_and(|state| state.is_ok()));
    }
}
