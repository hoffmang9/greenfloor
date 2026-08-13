use thiserror::Error;

/// HTTP, Coinset, and wallet-sdk driver failures.
#[derive(Debug, Error)]
pub enum TransportError {
    #[error("coinset error: {0}")]
    Coinset(String),
    #[error("http timeout ({layer}): {message}")]
    Timeout {
        layer: &'static str,
        message: String,
    },
    #[error("http connect ({layer}): {message}")]
    Connect {
        layer: &'static str,
        message: String,
    },
    #[error("http error ({layer}): {message}")]
    Http {
        layer: &'static str,
        message: String,
    },
    #[error("http status {status} ({layer}): {message}")]
    HttpStatus {
        layer: &'static str,
        status: u16,
        message: String,
    },
    #[error("driver error: {0}")]
    Driver(String),
}

impl TransportError {
    #[must_use]
    pub fn from_reqwest(layer: &'static str, err: &reqwest::Error) -> Self {
        let message = err.to_string();
        if err.is_timeout() {
            Self::Timeout { layer, message }
        } else if err.is_connect() {
            Self::Connect { layer, message }
        } else {
            Self::Http { layer, message }
        }
    }

    #[must_use]
    pub fn is_http_not_found(&self) -> bool {
        matches!(self, Self::HttpStatus { status: 404, .. })
    }

    #[must_use]
    pub fn is_parallel_dispatch_transient(&self) -> bool {
        matches!(self, Self::Timeout { .. } | Self::Connect { .. })
    }
}
