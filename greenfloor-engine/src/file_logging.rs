//! Process-wide file logging helpers (`{home_dir}/logs/debug.log`).
//!
//! **One global subscriber per process:** the first successful
//! [`sync_service_file_logging`] call installs the tracing registry. Daemon and manager each
//! keep a separate `OnceLock` slot, but they share one process-global subscriber — only one
//! service may init logging in a process. A second init fails if tracing is already global.
//! Production binaries (`greenfloord`, `greenfloor-manager`) are separate processes.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::reload::{self, Handle};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::error::{SignerError, SignerResult};

pub const DEFAULT_LOG_LEVEL: &str = "INFO";
pub const LOG_FILE: &str = "logs/debug.log";

pub const ALLOWED_LOG_LEVELS: &[&str] =
    &["CRITICAL", "ERROR", "WARNING", "INFO", "DEBUG", "NOTSET"];

pub(crate) struct LogState {
    home_dir: PathBuf,
    filter_handle: Handle<EnvFilter, tracing_subscriber::Registry>,
}

fn classify_log_level(log_level: &str) -> Option<&'static str> {
    match log_level.trim().to_ascii_uppercase().as_str() {
        "CRITICAL" => Some("CRITICAL"),
        "ERROR" => Some("ERROR"),
        "WARNING" => Some("WARNING"),
        "INFO" => Some("INFO"),
        "DEBUG" => Some("DEBUG"),
        "NOTSET" => Some("NOTSET"),
        _ => None,
    }
}

/// Normalize a log level for tracing filters and config defaults.
///
/// Unknown or empty values default to [`DEFAULT_LOG_LEVEL`] (`INFO`).
#[must_use]
pub fn normalize_log_level_name(log_level: &str) -> &'static str {
    classify_log_level(log_level).unwrap_or(DEFAULT_LOG_LEVEL)
}

/// Normalize a log level to an owned uppercase string, defaulting invalid values to `INFO`.
#[must_use]
pub fn normalize_log_level_string(log_level: &str) -> String {
    normalize_log_level_name(log_level).to_string()
}

/// Validate explicit operator log-level input (for example `set-log-level`).
///
/// # Errors
///
/// Returns an error when `log_level` is not one of [`ALLOWED_LOG_LEVELS`].
pub fn validate_log_level(log_level: &str) -> SignerResult<String> {
    classify_log_level(log_level)
        .map(str::to_string)
        .ok_or_else(|| {
            SignerError::Other(format!(
                "log level must be one of: {}",
                ALLOWED_LOG_LEVELS.join(", ")
            ))
        })
}

pub fn warn_if_log_level_auto_healed(log_level_was_missing: bool, program_config_path: &Path) {
    if log_level_was_missing {
        tracing::warn!(
            program_config = %program_config_path.display(),
            "program config missing app.log_level; defaulting to INFO"
        );
    }
}

fn env_filter_for_level(log_level: &str) -> EnvFilter {
    EnvFilter::new(normalize_log_level_name(log_level))
}

fn open_log_file(service_name: &str, home_dir: &Path) -> Result<std::fs::File, String> {
    let log_path = home_dir.join(LOG_FILE);
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            format!(
                "failed to create {service_name} log dir {}: {err}",
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
                "failed to open {service_name} log file {}: {err}",
                log_path.display()
            )
        })
}

fn global_subscriber_conflict_message(
    service_name: &str,
    init_err: impl std::fmt::Display,
) -> String {
    format!(
        "failed to install {service_name} file logging subscriber: {init_err} \
         (a global tracing subscriber is already active; {service_name} logging must initialize first)"
    )
}

fn install_service_file_logging(
    service_name: &str,
    home_dir: &Path,
    log_level: &str,
) -> Result<LogState, String> {
    if tracing::dispatcher::has_been_set() {
        return Err(global_subscriber_conflict_message(
            service_name,
            "tracing dispatcher already set before logging init",
        ));
    }

    let normalized = normalize_log_level_name(log_level);
    let log_path = home_dir.join(LOG_FILE);
    let file = open_log_file(service_name, home_dir)?;

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
                global_subscriber_conflict_message(service_name, err)
            } else {
                format!("failed to init {service_name} file logging subscriber: {err}")
            }
        })?;

    tracing::info!(
        service = service_name,
        log_path = %log_path.display(),
        log_level = normalized,
        "{service_name} file logging initialized"
    );

    Ok(LogState {
        home_dir: home_dir.to_path_buf(),
        filter_handle,
    })
}

fn reload_service_log_level(
    service_name: &str,
    state: &LogState,
    home_dir: &Path,
    log_level: &str,
) -> Result<(), String> {
    if state.home_dir != home_dir {
        tracing::warn!(
            service = service_name,
            active_home = %state.home_dir.display(),
            requested_home = %home_dir.display(),
            "{service_name} log file path unchanged after home_dir change; restart process to relocate logs"
        );
    }

    let normalized = normalize_log_level_name(log_level);
    state
        .filter_handle
        .modify(|filter| *filter = env_filter_for_level(log_level))
        .map_err(|err| format!("failed to reload {service_name} log filter: {err}"))?;

    tracing::debug!(
        service = service_name,
        log_level = normalized,
        "{service_name} log level reloaded"
    );
    Ok(())
}

/// Initialize or refresh `{service_name}` file logging for the current process.
///
/// The first call installs the file subscriber under `{home_dir}/logs/debug.log`.
/// Later calls update the active `EnvFilter` when `log_level` changes. The log file
/// path stays fixed after first init — a changed `home_dir` emits a warning until
/// process restart.
///
/// # Errors
///
/// Returns an error if the first initialization attempt fails, including when another
/// global tracing subscriber was installed first.
pub fn sync_service_file_logging(
    slot: &OnceLock<Result<LogState, String>>,
    service_name: &'static str,
    home_dir: &Path,
    log_level: &str,
) -> SignerResult<()> {
    if let Some(state) = slot.get() {
        return match state {
            Ok(active) => reload_service_log_level(service_name, active, home_dir, log_level)
                .map_err(SignerError::Other),
            Err(message) => Err(SignerError::Other(message.clone())),
        };
    }

    let state =
        slot.get_or_init(|| install_service_file_logging(service_name, home_dir, log_level));
    match state {
        Ok(_active) => Ok(()),
        Err(message) => Err(SignerError::Other(message.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_LOG_STATE: OnceLock<Result<LogState, String>> = OnceLock::new();

    #[test]
    fn normalize_log_level_defaults_invalid_to_info() {
        assert_eq!(normalize_log_level_name("debug"), "DEBUG");
        assert_eq!(normalize_log_level_name(""), DEFAULT_LOG_LEVEL);
        assert_eq!(normalize_log_level_name("verbose"), DEFAULT_LOG_LEVEL);
        assert_eq!(normalize_log_level_string("verbose"), "INFO");
    }

    #[test]
    fn validate_log_level_accepts_info_and_rejects_garbage() {
        assert_eq!(validate_log_level("info").expect("level"), "INFO");
        assert!(validate_log_level("verbose").is_err());
    }

    #[test]
    fn sync_service_file_logging_creates_log_file_and_reloads_level() {
        if tracing::dispatcher::has_been_set() {
            return;
        }

        let dir = tempfile::tempdir().expect("tempdir");
        sync_service_file_logging(&TEST_LOG_STATE, "test-service", dir.path(), "INFO")
            .expect("init");
        let log_path = dir.path().join(LOG_FILE);
        assert!(log_path.is_file());

        sync_service_file_logging(&TEST_LOG_STATE, "test-service", dir.path(), "DEBUG")
            .expect("reload");
        assert!(TEST_LOG_STATE.get().is_some_and(|state| state.is_ok()));
    }
}
