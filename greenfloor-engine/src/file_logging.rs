//! Process-wide file logging helpers (`{home_dir}/logs/debug.log`).
//!
//! **One global subscriber per process:** the first successful
//! [`sync_service_file_logging`] call installs the tracing registry. Additional services
//! in the same process either install their own slot or record
//! [`ServiceLogState::SharedProcess`] when tracing was already initialized elsewhere
//! (parallel unit tests). Production binaries (`greenfloord`, `greenfloor-manager`) are
//! separate processes.

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

/// Per-service logging slot state after the first [`sync_service_file_logging`] call.
pub(crate) enum ServiceLogState {
    Installed(LogState),
    /// Process-global tracing was already active (another service or parallel test init first).
    SharedProcess,
}

static LOG_TRACER: OnceLock<()> = OnceLock::new();

fn ensure_log_tracer_installed() {
    LOG_TRACER.get_or_init(|| {
        let _ = tracing_log::LogTracer::init();
    });
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

    ensure_log_tracer_installed();

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
/// When another component has already installed the process-global tracing subscriber,
/// subsequent services record [`ServiceLogState::SharedProcess`] and return success without
/// failing (parallel unit tests and multi-service dev harnesses).
///
/// # Errors
///
/// Returns an error if the first initialization attempt fails.
pub(crate) fn sync_service_file_logging(
    slot: &OnceLock<Result<ServiceLogState, String>>,
    service_name: &'static str,
    home_dir: &Path,
    log_level: &str,
) -> SignerResult<()> {
    if let Some(state) = slot.get() {
        return match state {
            Ok(ServiceLogState::Installed(active)) => {
                reload_service_log_level(service_name, active, home_dir, log_level)
                    .map_err(SignerError::Other)
            }
            Ok(ServiceLogState::SharedProcess) => Ok(()),
            Err(message) => Err(SignerError::Other(message.clone())),
        };
    }

    if tracing::dispatcher::has_been_set() {
        slot.get_or_init(|| Ok(ServiceLogState::SharedProcess));
        return Ok(());
    }

    let state = slot.get_or_init(|| {
        install_service_file_logging(service_name, home_dir, log_level)
            .map(ServiceLogState::Installed)
    });
    match state {
        Ok(_) => Ok(()),
        Err(message) => Err(SignerError::Other(message.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_LOG_STATE: OnceLock<Result<ServiceLogState, String>> = OnceLock::new();

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
    fn sync_service_file_logging_records_shared_process_when_subscriber_active() {
        static SHARED_SLOT: OnceLock<Result<ServiceLogState, String>> = OnceLock::new();
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        if !tracing::dispatcher::has_been_set() {
            return;
        }

        let dir = tempfile::tempdir().expect("tempdir");
        sync_service_file_logging(&SHARED_SLOT, "shared-service", dir.path(), "INFO")
            .expect("shared process init");
        assert!(matches!(
            SHARED_SLOT.get(),
            Some(Ok(ServiceLogState::SharedProcess))
        ));
        sync_service_file_logging(&SHARED_SLOT, "shared-service", dir.path(), "DEBUG")
            .expect("shared process reload noop");
    }

    #[test]
    fn sync_service_file_logging_creates_log_file_and_reloads_level() {
        if tracing::dispatcher::has_been_set() {
            return;
        }

        let dir = tempfile::tempdir().expect("tempdir");
        let init = sync_service_file_logging(&TEST_LOG_STATE, "test-service", dir.path(), "INFO");
        if init.is_err() && tracing::dispatcher::has_been_set() {
            return;
        }
        init.expect("init");
        let log_path = dir.path().join(LOG_FILE);
        assert!(log_path.is_file());

        sync_service_file_logging(&TEST_LOG_STATE, "test-service", dir.path(), "DEBUG")
            .expect("reload");
        assert!(TEST_LOG_STATE
            .get()
            .is_some_and(|state| { matches!(state, Ok(ServiceLogState::Installed(_))) }));

        crate::trace_event!(
            INFO,
            crate::operator_log::LogContext::DAEMON_CYCLE,
            crate::operator_log::DAEMON_CYCLE_STARTED,
            {
                market_count = 2,
                dry_run = false,
                selected_market_ids = ?["m1", "m2"],
            };
            "daemon cycle started"
        );

        let log = std::fs::read_to_string(&log_path).expect("read log");
        assert!(log.contains("daemon_cycle_started"));
        assert!(log.contains("market_count"));
        assert!(log.contains("m1"));
    }
}
