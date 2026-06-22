use std::path::Path;
use std::sync::OnceLock;

use crate::error::SignerResult;
use crate::file_logging::{self, ServiceLogState, DEFAULT_LOG_LEVEL};

static LOG_STATE: OnceLock<Result<ServiceLogState, String>> = OnceLock::new();

const SERVICE_NAME: &str = "daemon";

pub use crate::file_logging::warn_if_log_level_auto_healed;

/// Initialize or refresh daemon file logging for the current process.
///
/// The first call installs the file subscriber under `{home_dir}/logs/debug.log`.
/// Later calls update the active `EnvFilter` when `log_level` changes (config reload /
/// `set-log-level`). The log file path stays fixed after first init — a changed `home_dir`
/// emits a warning until process restart.
///
/// # Errors
///
/// Returns an error if the first initialization attempt fails.
pub fn sync_daemon_file_logging(home_dir: &Path, log_level: &str) -> SignerResult<()> {
    file_logging::sync_service_file_logging(&LOG_STATE, SERVICE_NAME, home_dir, log_level)
}

#[must_use]
pub fn default_log_level() -> &'static str {
    DEFAULT_LOG_LEVEL
}
