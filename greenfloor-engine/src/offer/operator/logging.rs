use std::path::Path;
use std::sync::OnceLock;

use crate::error::SignerResult;
use crate::file_logging::{self, LogState};

static LOG_STATE: OnceLock<Result<LogState, String>> = OnceLock::new();

const SERVICE_NAME: &str = "manager";

pub use crate::file_logging::warn_if_log_level_auto_healed;

/// Initialize or refresh manager file logging for the current process.
///
/// The first call installs the file subscriber under `{home_dir}/logs/debug.log`.
/// Later calls update the active `EnvFilter` when `log_level` changes (for example after
/// `set-log-level`). The log file path stays fixed after first init — a changed `home_dir`
/// emits a warning until process restart.
///
/// # Errors
///
/// Returns an error if the first initialization attempt fails, including when another
/// global tracing subscriber was installed first.
pub fn sync_manager_file_logging(home_dir: &Path, log_level: &str) -> SignerResult<()> {
    file_logging::sync_service_file_logging(&LOG_STATE, SERVICE_NAME, home_dir, log_level)
}

/// Initialize manager file logging once per process.
///
/// Alias for [`sync_manager_file_logging`].
///
/// # Errors
///
/// Returns an error if initialization fails.
pub fn initialize_manager_file_logging(home_dir: &Path, log_level: &str) -> SignerResult<()> {
    sync_manager_file_logging(home_dir, log_level)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_logging::LOG_FILE;

    #[test]
    fn sync_manager_file_logging_creates_log_file_and_reloads_level() {
        if tracing::dispatcher::has_been_set() {
            return;
        }

        let dir = tempfile::tempdir().expect("tempdir");
        let init = sync_manager_file_logging(dir.path(), "INFO");
        if init.is_err() && tracing::dispatcher::has_been_set() {
            return;
        }
        init.expect("init");
        let log_path = dir.path().join(LOG_FILE);
        assert!(log_path.is_file());

        sync_manager_file_logging(dir.path(), "DEBUG").expect("reload");
        assert!(LOG_STATE.get().is_some_and(|state| state.is_ok()));
    }
}
