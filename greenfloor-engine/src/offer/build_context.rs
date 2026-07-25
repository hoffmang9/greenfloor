use crate::config::MarketPricing;
use crate::error::SignerResult;

/// Resolve offer expiry unit and value from market pricing (minutes-only contract).
#[must_use]
pub fn resolve_offer_expiry_for_pricing(pricing: &MarketPricing) -> (&'static str, i64) {
    ("minutes", pricing.offer_expiry_minutes())
}

/// Resolve quote-per-base for manual offer build from market pricing.
///
/// # Errors
///
/// Returns an error when pricing lacks a usable quote price.
pub fn resolve_quote_price_for_pricing(pricing: &MarketPricing) -> SignerResult<f64> {
    pricing.quote_price()
}

/// Mojo multiplier for a market leg using typed pricing then asset default.
#[must_use]
pub fn mojo_multiplier_for_leg(pricing: &MarketPricing, field: &str, asset_id: &str) -> i64 {
    pricing.mojo_multiplier_for_leg(field, asset_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MarketPricing;

    #[test]
    fn expiry_defaults_to_ten_minutes() {
        let (unit, value) = resolve_offer_expiry_for_pricing(&MarketPricing::default());
        assert_eq!(unit, "minutes");
        assert_eq!(value, 10);
    }

    #[test]
    fn expiry_uses_configured_minutes() {
        let pricing = MarketPricing {
            strategy_offer_expiry_minutes: Some(12),
            ..MarketPricing::default()
        };
        let (unit, value) = resolve_offer_expiry_for_pricing(&pricing);
        assert_eq!(unit, "minutes");
        assert_eq!(value, 12);
    }

    #[test]
    fn quote_price_midpoint_from_min_max() {
        let pricing = MarketPricing {
            min_price_quote_per_base: Some(1.0),
            max_price_quote_per_base: Some(3.0),
            ..MarketPricing::default()
        };
        let price = resolve_quote_price_for_pricing(&pricing).unwrap();
        assert!((price - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn quote_price_requires_pricing_fields() {
        assert!(resolve_quote_price_for_pricing(&MarketPricing::default()).is_err());
    }
}
