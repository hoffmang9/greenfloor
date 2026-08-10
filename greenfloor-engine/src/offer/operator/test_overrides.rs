//! Test-only overrides for offer operator dry-run and preview paths.
//!
//! Canonical pattern: see [`crate::test_support::injections`].

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[cfg(test)]
pub struct BuildOfferTestOverrides {
    #[serde(default)]
    pub offer_text: Option<String>,
    /// When set, unique-maker live pin returns this result (no Coinset call).
    #[serde(default, skip)]
    pub unique_pin_result: Option<Result<String, String>>,
}

#[cfg(test)]
impl BuildOfferTestOverrides {
    pub(crate) fn stub_offer_text(&self) -> Option<&str> {
        self.offer_text.as_deref()
    }
}
