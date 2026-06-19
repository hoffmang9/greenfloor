//! Shared retry helper for script-style Coinset HTTP calls.

use std::time::Duration;

use rand::Rng;

use crate::cli_util::script_engine_error_retryable;
use crate::error::{SignerError, SignerResult};

const MAX_ATTEMPTS: usize = 4;

/// With script retries.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn with_script_retries<T, F, Fut>(mut operation: F) -> SignerResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = SignerResult<T>>,
{
    let mut delay = 0.8f64;
    for attempt in 1..=MAX_ATTEMPTS {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(err) if attempt < MAX_ATTEMPTS && script_engine_error_retryable(&err) => {
                let jitter = rand::rng().random_range(-0.25..=0.25);
                tokio::time::sleep(Duration::from_secs_f64((delay * (1.0 + jitter)).max(0.05)))
                    .await;
                delay = (delay * 2.0).min(8.0);
            }
            Err(err) => return Err(err),
        }
    }
    Err(SignerError::Other(
        "coinset retry logic unreachable".to_string(),
    ))
}
