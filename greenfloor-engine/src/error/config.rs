use thiserror::Error;

/// Program/markets `YAML` and operator-path configuration failures.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing config field: {0}")]
    MissingField(&'static str),

    #[error("offer execution requires signer.kms_key_id and vault.launcher_id in program config")]
    SignerPathNotConfigured,

    #[error("daemon_already_running:{path}{detail}")]
    DaemonAlreadyRunning { path: String, detail: String },

    #[error("{0}")]
    Parse(String),
}
