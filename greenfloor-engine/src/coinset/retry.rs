//! Shared retry helper for script-style Coinset HTTP calls.

use std::future::Future;
use std::time::Duration;

use rand::Rng;

use crate::cli_util::script_engine_error_retryable;
use crate::error::{SignerError, SignerResult};

/// Backoff policy for [`with_script_retries`].
///
/// Production builds default to [`Self::PRODUCTION`]. Lib unit-test builds
/// (`cfg(test)`) default to [`Self::UNIT_TEST`] so failure-path tests do not wait
/// on multi-second production backoff. Pass an explicit policy when a test must
/// exercise production timing or a custom attempt count.
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

    /// Default policy for [`with_script_retries`] / [`with_coinset_client_retries`].
    #[cfg(test)]
    #[must_use]
    pub const fn default_for_build() -> Self {
        Self::UNIT_TEST
    }

    /// Default policy for [`with_script_retries`] / [`with_coinset_client_retries`].
    #[cfg(not(test))]
    #[must_use]
    pub const fn default_for_build() -> Self {
        Self::PRODUCTION
    }
}

fn retry_sleep_duration(policy: ScriptRetryPolicy, delay: f64) -> Duration {
    let jitter = rand::rng().random_range(-0.25..=0.25);
    let scaled = (delay * policy.sleep_scale * (1.0 + jitter)).max(policy.min_sleep_secs);
    Duration::from_secs_f64(scaled)
}

/// With script retries using [`ScriptRetryPolicy::default_for_build`].
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn with_script_retries<T, F, Fut>(operation: F) -> SignerResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = SignerResult<T>>,
{
    with_script_retries_with_policy(ScriptRetryPolicy::default_for_build(), operation).await
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

/// Retry a [`CoinsetClient`] RPC using [`ScriptRetryPolicy::default_for_build`].
///
/// # Errors
///
/// Returns an error if the operation fails after retries are exhausted.
pub async fn with_coinset_client_retries<T, F, Fut>(operation: F) -> SignerResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, reqwest::Error>>,
{
    with_coinset_client_retries_with_policy(ScriptRetryPolicy::default_for_build(), operation).await
}

/// Retry a [`CoinsetClient`] RPC with an explicit backoff policy.
///
/// # Errors
///
/// Returns an error if the operation fails after retries are exhausted.
pub async fn with_coinset_client_retries_with_policy<T, F, Fut>(
    policy: ScriptRetryPolicy,
    mut operation: F,
) -> SignerResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, reqwest::Error>>,
{
    with_script_retries_with_policy(policy, || {
        let future = operation();
        async move { future.await.map_err(SignerError::from) }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_policy_matches_operator_defaults() {
        assert_eq!(ScriptRetryPolicy::PRODUCTION.max_attempts, 4);
        assert_eq!(
            ScriptRetryPolicy::default_for_build().max_attempts,
            ScriptRetryPolicy::UNIT_TEST.max_attempts
        );
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

    #[tokio::test]
    async fn with_coinset_client_retries_retries_transient_reqwest_errors() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let attempts = Arc::new(AtomicUsize::new(0));
        let value = with_coinset_client_retries_with_policy(ScriptRetryPolicy::UNIT_TEST, {
            let attempts = Arc::clone(&attempts);
            move || {
                let attempt = attempts.fetch_add(1, Ordering::SeqCst);
                async move {
                    if attempt == 0 {
                        reqwest::Client::new()
                            .get("http://127.0.0.1:1")
                            .send()
                            .await
                            .map(|_| 0u32)
                    } else {
                        Ok(7u32)
                    }
                }
            }
        })
        .await
        .expect("retry succeeds");
        assert_eq!(value, 7);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }
}
