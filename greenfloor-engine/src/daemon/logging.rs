use std::path::Path;
use std::sync::Once;

use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use crate::error::{SignerError, SignerResult};

const SERVICE_NAME: &str = "daemon";
const DEFAULT_LOG_LEVEL: &str = "INFO";
const LOG_FILE: &str = "logs/debug.log";

static INIT: Once = Once::new();

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

pub fn initialize_daemon_file_logging(home_dir: &Path, log_level: &str) -> SignerResult<()> {
    let normalized = normalize_log_level_name(log_level);
    let log_path = home_dir.join(LOG_FILE);
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            SignerError::Other(format!(
                "failed to create daemon log dir {}: {err}",
                parent.display()
            ))
        })?;
    }

    INIT.call_once(|| {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .expect("daemon log file should open after parent dir creation");
        let file_layer = tracing_subscriber::fmt::layer()
            .with_writer(file)
            .with_ansi(false)
            .with_target(true)
            .with_level(true)
            .with_span_events(FmtSpan::NONE)
            .with_filter(EnvFilter::new(normalized));
        let _ = tracing_subscriber::registry().with(file_layer).try_init();
    });

    tracing::info!(
        service = SERVICE_NAME,
        log_path = %log_path.display(),
        log_level = normalized,
        "daemon file logging initialized"
    );
    Ok(())
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

pub fn default_log_level() -> &'static str {
    DEFAULT_LOG_LEVEL
}
