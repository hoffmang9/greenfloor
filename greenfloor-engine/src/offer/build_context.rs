use serde_json::Value;

use crate::error::{SignerError, SignerResult};
use crate::hex::default_mojo_multiplier_for_asset;

const DEFAULT_OFFER_EXPIRY_MINUTES: i64 = 10;

/// Resolve offer expiry unit and value from market ``pricing`` (minutes-only contract).
pub fn resolve_offer_expiry_for_pricing(pricing: &Value) -> (&'static str, i64) {
    let minutes = pricing
        .get("strategy_offer_expiry_minutes")
        .and_then(value_as_i64)
        .unwrap_or(0);
    if minutes > 0 {
        return ("minutes", minutes);
    }
    ("minutes", DEFAULT_OFFER_EXPIRY_MINUTES)
}

/// Resolve quote-per-base for manual offer build from market ``pricing``.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn resolve_quote_price_for_pricing(pricing: &Value) -> SignerResult<f64> {
    if let Some(fixed) = pricing.get("fixed_quote_per_base") {
        if let Some(price) = value_as_f64(fixed) {
            return Ok(price);
        }
    }
    let min_q = pricing
        .get("min_price_quote_per_base")
        .and_then(value_as_f64);
    let max_q = pricing
        .get("max_price_quote_per_base")
        .and_then(value_as_f64);
    let quote_price: Option<f64> = match (min_q, max_q) {
        (Some(min), Some(max)) => Some(f64::midpoint(min, max)),
        (Some(min), None) => Some(min),
        (None, Some(max)) => Some(max),
        (None, None) => None,
    };
    quote_price.ok_or_else(|| {
        SignerError::Other(
            "market pricing must define fixed_quote_per_base or \
             min/max_price_quote_per_base for offer build"
                .to_string(),
        )
    })
}

/// Mojo multiplier for a market leg using optional pricing override then asset default.
pub fn mojo_multiplier_for_leg(pricing: &Value, field: &str, asset_id: &str) -> i64 {
    pricing
        .get(field)
        .and_then(value_as_i64)
        .unwrap_or_else(|| default_mojo_multiplier_for_asset(asset_id))
}

fn value_as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn value_as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn expiry_defaults_to_ten_minutes() {
        let (unit, value) = resolve_offer_expiry_for_pricing(&json!({}));
        assert_eq!(unit, "minutes");
        assert_eq!(value, 10);
    }

    #[test]
    fn expiry_uses_configured_minutes() {
        let (unit, value) =
            resolve_offer_expiry_for_pricing(&json!({"strategy_offer_expiry_minutes": 12}));
        assert_eq!(unit, "minutes");
        assert_eq!(value, 12);
    }

    #[test]
    fn quote_price_midpoint_from_min_max() {
        let price = resolve_quote_price_for_pricing(&json!({
            "min_price_quote_per_base": 1.0,
            "max_price_quote_per_base": 3.0,
        }))
        .unwrap();
        assert!((price - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn quote_price_requires_pricing_fields() {
        assert!(resolve_quote_price_for_pricing(&json!({})).is_err());
    }
}
