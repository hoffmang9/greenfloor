use std::future::Future;
use std::time::{Duration, Instant};

use crate::error::{SignerError, SignerResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PollConfig {
    pub timeout: Duration,
    pub interval: Duration,
}

impl PollConfig {
    pub(crate) fn from_seconds(timeout_seconds: u64, interval_seconds: u64) -> Self {
        Self {
            timeout: Duration::from_secs(timeout_seconds.max(1)),
            interval: Duration::from_secs(interval_seconds.max(1)),
        }
    }

    /// Sub-second poll loop for unit tests (same pattern as
    /// [`crate::daemon::coinset_ws::OnceCaptureTimings::UNIT_TEST`]).
    #[cfg(test)]
    pub(crate) const UNIT_TEST: Self = Self {
        timeout: Duration::from_millis(40),
        interval: Duration::from_millis(1),
    };
}

pub(crate) async fn run_poll_loop<F, Fut, T>(
    mut attempt: F,
    config: PollConfig,
    on_timeout: SignerError,
) -> SignerResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = SignerResult<Option<T>>>,
{
    let started = Instant::now();
    loop {
        if let Some(value) = attempt().await? {
            return Ok(value);
        }
        if started.elapsed() >= config.timeout {
            return Err(on_timeout);
        }
        tokio::time::sleep(config.interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[tokio::test]
    async fn run_poll_loop_returns_when_attempt_succeeds() {
        let attempts = Rc::new(Cell::new(0u8));
        let value = run_poll_loop(
            {
                let attempts = Rc::clone(&attempts);
                move || {
                    attempts.set(attempts.get() + 1);
                    let attempts = Rc::clone(&attempts);
                    async move {
                        if attempts.get() < 2 {
                            Ok(None)
                        } else {
                            Ok(Some(42))
                        }
                    }
                }
            },
            PollConfig {
                timeout: Duration::from_millis(50),
                interval: Duration::from_millis(1),
            },
            SignerError::CombineInputVerifyTimeout,
        )
        .await
        .expect("poll");
        assert_eq!(value, 42);
        assert_eq!(attempts.get(), 2);
    }

    #[tokio::test]
    async fn run_poll_loop_returns_timeout_error() {
        let err = run_poll_loop(
            || async { Ok(None::<i32>) },
            PollConfig {
                timeout: Duration::from_millis(5),
                interval: Duration::from_millis(1),
            },
            SignerError::CombineInputVerifyTimeout,
        )
        .await
        .expect_err("timeout");
        assert!(matches!(err, SignerError::CombineInputVerifyTimeout));
    }
}
