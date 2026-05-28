//! Shared transient-retry and polling backoff policy for HTTP adapters.

const RATE_LIMIT_PATTERN: &str = "try again in ";

/// Parse Dexie/Cloud Wallet rate-limit hint: ``try again in N seconds``.
pub fn parse_rate_limit_retry_seconds(error_text: &str) -> Option<f64> {
    let lower = error_text.to_ascii_lowercase();
    let idx = lower.find(RATE_LIMIT_PATTERN)?;
    let rest = &lower[idx + RATE_LIMIT_PATTERN.len()..];
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<i64>().ok().map(|seconds| seconds as f64)
}

/// Next sleep for generic moderate retry (attempt is 1-based after a failure).
pub fn moderate_retry_sleep_seconds(
    attempt: u32,
    mut current_sleep: f64,
    rate_limit_wait: Option<f64>,
) -> f64 {
    if let Some(wait) = rate_limit_wait {
        current_sleep = current_sleep.max((wait + 0.25).min(30.0));
    }
    let _ = attempt;
    current_sleep
}

/// Advance sleep for the next moderate-retry attempt (returns updated baseline).
pub fn moderate_retry_next_sleep(current_sleep: f64) -> f64 {
    (current_sleep * 2.0).min(8.0)
}

pub fn dexie_invalid_offer_should_retry(error: &str, attempt: u32, max_attempts: u32) -> bool {
    let normalized = error.trim();
    normalized.contains("dexie_http_error:400")
        && normalized.contains("Invalid Offer")
        && attempt < max_attempts.saturating_sub(1)
}

pub fn dexie_invalid_offer_retry_sleep(attempt: u32, initial_sleep: f64) -> f64 {
    let multiplier = 2f64.powi(attempt.min(31) as i32);
    (initial_sleep * multiplier).min(8.0)
}

pub fn coinset_fee_lookup_retry_sleep(attempt: u32) -> f64 {
    (0.5 * 2f64.powi(attempt.min(31) as i32)).min(8.0)
}

/// Returns ``Some(next_sleep)`` while polling should continue, or ``None`` when timed out.
pub fn poll_exponential_next_sleep(
    elapsed_seconds: i64,
    timeout_seconds: i64,
    current_sleep: f64,
    initial_sleep: f64,
    max_sleep: f64,
    multiplier: f64,
) -> Option<f64> {
    if elapsed_seconds >= timeout_seconds {
        return None;
    }
    Some(if current_sleep <= 0.0 {
        initial_sleep
    } else {
        (current_sleep * multiplier).min(max_sleep)
    })
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
    fn poll_exponential_times_out() {
        assert!(poll_exponential_next_sleep(10, 10, 1.0, 0.5, 8.0, 2.0).is_none());
        assert_eq!(
            poll_exponential_next_sleep(0, 10, 1.0, 0.5, 8.0, 2.0),
            Some(2.0)
        );
    }
}
