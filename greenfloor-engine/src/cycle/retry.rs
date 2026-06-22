//! Shared transient-retry and polling backoff policy for HTTP adapters.

const RATE_LIMIT_PATTERN: &str = "try again in ";
const MAX_BACKOFF_ATTEMPT_SHIFT: u32 = 31;

fn exponential_multiplier(attempt: u32) -> f64 {
    2f64.powi(i32::try_from(attempt.min(MAX_BACKOFF_ATTEMPT_SHIFT)).unwrap_or(31))
}

#[must_use]
pub fn exponential_backoff_f64(base: f64, attempt: u32, cap: f64) -> f64 {
    (base * exponential_multiplier(attempt)).min(cap)
}

/// Parse Dexie/Cloud Wallet rate-limit hint: ``try again in N seconds``.
pub fn parse_rate_limit_retry_seconds(error_text: &str) -> Option<f64> {
    let lower = error_text.to_ascii_lowercase();
    let idx = lower.find(RATE_LIMIT_PATTERN)?;
    let rest = &lower[idx + RATE_LIMIT_PATTERN.len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits
        .parse::<i64>()
        .ok()
        .map(crate::offer::pricing::i64_to_f64)
}

/// Sleep duration before the next moderate-retry attempt after a failure.
#[must_use]
pub fn moderate_retry_sleep_seconds(mut current_sleep: f64, rate_limit_wait: Option<f64>) -> f64 {
    if let Some(wait) = rate_limit_wait {
        current_sleep = current_sleep.max((wait + 0.25).min(30.0));
    }
    current_sleep
}

/// Advance sleep for the next moderate-retry attempt (returns updated baseline).
#[must_use]
pub fn moderate_retry_next_sleep(current_sleep: f64) -> f64 {
    (current_sleep * 2.0).min(8.0)
}

#[must_use]
pub fn dexie_invalid_offer_should_retry(error: &str, attempt: u32, max_attempts: u32) -> bool {
    let normalized = error.trim();
    normalized.contains("dexie_http_error:400")
        && normalized.contains("Invalid Offer")
        && attempt < max_attempts.saturating_sub(1)
}

#[must_use]
pub fn dexie_invalid_offer_retry_sleep(attempt: u32, initial_sleep: f64) -> f64 {
    exponential_backoff_f64(initial_sleep, attempt, 8.0)
}

#[must_use]
pub fn coinset_fee_lookup_retry_sleep(attempt: u32) -> f64 {
    exponential_backoff_f64(0.5, attempt, 8.0)
}

/// Sleep duration to use after a failed poll tick, or ``None`` when timed out.
#[must_use]
pub fn poll_exponential_sleep_now(
    elapsed_seconds: i64,
    timeout_seconds: i64,
    sleep_seconds: f64,
    initial_sleep: f64,
    max_sleep: f64,
) -> Option<f64> {
    if elapsed_seconds >= timeout_seconds {
        return None;
    }
    Some(if sleep_seconds <= 0.0 {
        initial_sleep
    } else {
        sleep_seconds.min(max_sleep)
    })
}

/// Advance sleep baseline after a poll backoff sleep.
#[must_use]
pub fn poll_exponential_advance_sleep(
    sleep_seconds: f64,
    initial_sleep: f64,
    max_sleep: f64,
    multiplier: f64,
) -> f64 {
    let base = if sleep_seconds <= 0.0 {
        initial_sleep
    } else {
        sleep_seconds
    };
    (base * multiplier).min(max_sleep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rate_limit_seconds_case_insensitive() {
        assert_eq!(
            parse_rate_limit_retry_seconds("Try Again In 12 Seconds"),
            Some(12.0)
        );
        assert_eq!(parse_rate_limit_retry_seconds("rate limited"), None);
    }

    #[test]
    fn dexie_invalid_offer_retry_gates() {
        let err = r#"dexie_http_error:400:{"error_message":"Invalid Offer"}"#;
        assert!(dexie_invalid_offer_should_retry(err, 0, 4));
        assert!(!dexie_invalid_offer_should_retry(err, 3, 4));
    }

    #[test]
    fn poll_exponential_sleep_and_advance() {
        assert!(poll_exponential_sleep_now(10, 10, 1.0, 0.5, 8.0).is_none());
        assert_eq!(poll_exponential_sleep_now(0, 10, 0.0, 0.5, 8.0), Some(0.5));
        assert!((poll_exponential_advance_sleep(0.5, 0.5, 8.0, 2.0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn exponential_backoff_doubles_until_cap() {
        assert!((exponential_backoff_f64(0.25, 1, 8.0) - 0.5).abs() < f64::EPSILON);
        assert!((exponential_backoff_f64(0.25, 10, 8.0) - 8.0).abs() < f64::EPSILON);
    }
}
