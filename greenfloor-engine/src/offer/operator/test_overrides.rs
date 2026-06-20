//! Test-only overrides for offer operator dry-run and preview paths.

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct OfferOperatorTestOverrides {
    #[serde(default)]
    pub offer_text: Option<String>,
}

impl OfferOperatorTestOverrides {
    #[cfg(test)]
    #[must_use]
    pub fn from_env() -> Self {
        let offer_text = std::env::var("GREENFLOOR_TEST_OFFER_TEXT")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        Self { offer_text }
    }
}
