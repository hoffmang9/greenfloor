//! Retry-backed helpers for [`CoinsetClient`] RPC on wallet listing paths.

use std::future::Future;

use crate::error::{SignerError, SignerResult};

use super::retry::{with_script_retries_with_policy, ScriptRetryPolicy};

/// Retry a [`CoinsetClient`] RPC using the production script backoff policy.
pub async fn with_client_retries<T, F, Fut>(operation: F) -> SignerResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, reqwest::Error>>,
{
    with_client_retries_with_policy(ScriptRetryPolicy::PRODUCTION, operation).await
}

/// Retry a [`CoinsetClient`] RPC with an explicit backoff policy.
pub async fn with_client_retries_with_policy<T, F, Fut>(
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::*;

    #[tokio::test]
    async fn with_client_retries_retries_transient_reqwest_errors() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let value = with_client_retries_with_policy(ScriptRetryPolicy::UNIT_TEST, {
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
