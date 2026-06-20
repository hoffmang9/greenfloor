//! Shared retry helper for script-style Coinset HTTP calls.

use std::time::Duration;

use rand::Rng;

use crate::cli_util::script_engine_error_retryable;
use crate::error::{SignerError, SignerResult};

/// Backoff policy for [`with_script_retries`].
///
/// Production callers should use [`Self::PRODUCTION`]. Unit tests that exercise retry loops
/// should pass [`Self::UNIT_TEST`] to [`with_script_retries_with_policy`].
#[derive(Debug, Clone, Copy)]
pub struct ScriptRetryPolicy {
    pub max_attempts: usize,
    pub sleep_scale: f64,
    pub min_sleep_secs: f64,
}

impl ScriptRetryPolicy {
    pub const PRODUCTION: Self = Self {
        max_attempts: 4,
        sleep_scale: 1.0,
        min_sleep_secs: 0.05,
    };

    /// Fast backoff for unit tests that intentionally hit retry paths.
    pub const UNIT_TEST: Self = Self {
        max_attempts: 2,
        sleep_scale: 0.01,
        min_sleep_secs: 0.001,
    };
}

fn retry_sleep_duration(policy: ScriptRetryPolicy, delay: f64) -> Duration {
    let jitter = rand::rng().random_range(-0.25..=0.25);
    let scaled = (delay * policy.sleep_scale * (1.0 + jitter)).max(policy.min_sleep_secs);
    Duration::from_secs_f64(scaled)
}

/// With script retries using the production backoff policy.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn with_script_retries<T, F, Fut>(operation: F) -> SignerResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = SignerResult<T>>,
{
    with_script_retries_with_policy(ScriptRetryPolicy::PRODUCTION, operation).await
}

/// With script retries using an explicit backoff policy.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn with_script_retries_with_policy<T, F, Fut>(
    policy: ScriptRetryPolicy,
    mut operation: F,
) -> SignerResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = SignerResult<T>>,
{
    let mut delay = 0.8f64;
    for attempt in 1..=policy.max_attempts {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(err) if attempt < policy.max_attempts && script_engine_error_retryable(&err) => {
                tokio::time::sleep(retry_sleep_duration(policy, delay)).await;
                delay = (delay * 2.0).min(8.0);
            }
            Err(err) => return Err(err),
        }
    }
    Err(SignerError::Other(
        "coinset retry logic unreachable".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_policy_matches_operator_defaults() {
        assert_eq!(ScriptRetryPolicy::PRODUCTION.max_attempts, 4);
    }

    #[tokio::test]
    async fn unit_test_policy_retries_once_before_success() {
        let mut attempts = 0;
        let value = with_script_retries_with_policy(ScriptRetryPolicy::UNIT_TEST, || {
            attempts += 1;
            async move {
                if attempts == 1 {
                    Err(SignerError::Coinset(
                        "error sending request for url (http://127.0.0.1:1/): connection refused"
                            .to_string(),
                    ))
                } else {
                    Ok("ok")
                }
            }
        })
        .await
        .expect("retry succeeds");
        assert_eq!(value, "ok");
        assert_eq!(attempts, 2);
    }
}
