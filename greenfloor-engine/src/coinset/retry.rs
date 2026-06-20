//! Shared retry helper for script-style Coinset HTTP calls.

use std::time::Duration;

use rand::Rng;

use crate::cli_util::script_engine_error_retryable;
use crate::error::{SignerError, SignerResult};

const MAX_ATTEMPTS: usize = 4;

#[cfg(test)]
const TEST_MAX_ATTEMPTS: usize = 2;

fn max_retry_attempts() -> usize {
    #[cfg(test)]
    {
        return TEST_MAX_ATTEMPTS;
    }
    #[cfg(not(test))]
    MAX_ATTEMPTS
}

fn retry_sleep_duration(delay: f64) -> Duration {
    let jitter = rand::rng().random_range(-0.25..=0.25);
    let scaled = {
        #[cfg(test)]
        {
            (delay * 0.01 * (1.0 + jitter)).max(0.001)
        }
        #[cfg(not(test))]
        {
            (delay * (1.0 + jitter)).max(0.05)
        }
    };
    Duration::from_secs_f64(scaled)
}

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
    for attempt in 1..=max_retry_attempts() {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(err) if attempt < max_retry_attempts() && script_engine_error_retryable(&err) => {
                tokio::time::sleep(retry_sleep_duration(delay)).await;
                delay = (delay * 2.0).min(8.0);
            }
            Err(err) => return Err(err),
        }
    }
    Err(SignerError::Other(
        "coinset retry logic unreachable".to_string(),
    ))
}
