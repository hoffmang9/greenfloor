use crate::error::SignerResult;

use super::request::DaemonCycleTestControls;

/// Env gate for non-default `test_controls` on `greenfloor-engine daemon-once`.
pub fn daemon_test_controls_enabled() -> bool {
    std::env::var("GREENFLOOR_DAEMON_TEST_CONTROLS")
        .ok()
        .is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        })
}

impl DaemonCycleTestControls {
    /// True when env-gated `daemon-once` test controls differ from defaults.
    ///
    /// `offer_dispatch` is intentionally excluded: it is `#[cfg(test)]` and
    /// `#[serde(skip)]`, so it is only set in-process by unit tests (via
    /// `offer_dispatch::tests::harness::set_offer_dispatch`) and never
    /// deserialized from CLI JSON. Dispatch injections therefore bypass
    /// `GREENFLOOR_DAEMON_TEST_CONTROLS` by design.
    #[must_use]
    pub fn is_non_default(&self) -> bool {
        self.skip_strategy_execution || self.force_market_error_for.is_some()
    }

    /// Ensure allowed.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn ensure_allowed(&self) -> SignerResult<()> {
        if self.is_non_default() && !daemon_test_controls_enabled() {
            return Err(crate::error::SignerError::Other(
                "non-default daemon test_controls require GREENFLOOR_DAEMON_TEST_CONTROLS=1"
                    .to_string(),
            ));
        }
        Ok(())
    }
}
