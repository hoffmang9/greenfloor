//! Debug-build integration-test overrides (populated at operator context construction only).

#[derive(Debug, Clone, Default)]
pub struct OfferOperatorTestOverrides {
    pub offer_text: Option<String>,
}

impl OfferOperatorTestOverrides {
    pub fn from_env() -> Self {
        #[cfg(debug_assertions)]
        {
            let offer_text = std::env::var("GREENFLOOR_TEST_OFFER_TEXT")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            Self { offer_text }
        }
        #[cfg(not(debug_assertions))]
        {
            Self::default()
        }
    }
}
