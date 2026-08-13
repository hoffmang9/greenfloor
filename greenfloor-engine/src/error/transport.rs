use thiserror::Error;

/// HTTP, Coinset, and wallet-sdk driver failures.
#[derive(Debug, Error)]
pub enum TransportError {
    #[error("coinset error: {0}")]
    Coinset(String),
    #[error("http error ({layer}): {message}")]
    Http {
        layer: &'static str,
        message: String,
    },
    #[error("driver error: {0}")]
    Driver(String),
}
