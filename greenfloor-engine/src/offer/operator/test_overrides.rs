//! Test-only overrides for offer operator dry-run and preview paths.

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
#[cfg(test)]
pub struct BuildOfferTestOverrides {
    #[serde(default)]
    pub offer_text: Option<String>,
}

#[cfg(test)]
impl BuildOfferTestOverrides {
    pub(crate) fn stub_offer_text(&self) -> Option<&str> {
        self.offer_text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}
