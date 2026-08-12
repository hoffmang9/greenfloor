//! Typed market pricing parsed from YAML/JSON.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::config::yaml_fields::{parse_f64_field, parse_i64_field};
use crate::error::{SignerError, SignerResult};
use crate::hex::default_mojo_multiplier_for_asset;
use crate::operator_log::{LogContext, MARKET_VALIDATION_WARNING};

const CANONICAL_CAT_UNIT_MOJOS: i64 = 1000;
const CANONICAL_XCH_UNIT_MOJOS: i64 = 1_000_000_000_000;
const DEFAULT_OFFER_EXPIRY_MINUTES: i64 = 10;

/// Strategy and denomination fields owned by markets config.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MarketPricing {
    /// `None` means use the asset default at read time; parse always sets `Some`.
    pub base_unit_mojo_multiplier: Option<i64>,
    /// `None` means use the asset default at read time; parse always sets `Some`.
    pub quote_unit_mojo_multiplier: Option<i64>,
    pub cancel_policy_stable_vs_unstable: bool,
    pub strategy_target_spread_bps: Option<i64>,
    pub strategy_min_xch_price_usd: Option<f64>,
    pub strategy_max_xch_price_usd: Option<f64>,
    pub strategy_offer_expiry_minutes: Option<i64>,
    pub fixed_quote_per_base: Option<f64>,
    pub min_price_quote_per_base: Option<f64>,
    pub max_price_quote_per_base: Option<f64>,
    pub side: Option<String>,
}

impl MarketPricing {
    #[must_use]
    pub fn action_side(&self) -> &str {
        self.side
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("sell")
    }

    #[must_use]
    pub fn offer_expiry_minutes(&self) -> i64 {
        self.strategy_offer_expiry_minutes
            .filter(|minutes| *minutes > 0)
            .unwrap_or(DEFAULT_OFFER_EXPIRY_MINUTES)
    }

    /// Mid quote-per-base for manual / managed offer build (no bid-ask offset).
    ///
    /// # Errors
    ///
    /// Returns an error when no usable quote price fields are present.
    pub fn quote_price(&self) -> SignerResult<f64> {
        if let Some(price) = self.fixed_quote_per_base.filter(|price| *price > 0.0) {
            return Ok(price);
        }
        match (self.min_price_quote_per_base, self.max_price_quote_per_base) {
            (Some(min), Some(max)) => Ok(f64::midpoint(min, max)),
            (Some(min), None) => Ok(min),
            (None, Some(max)) => Ok(max),
            (None, None) => Err(SignerError::Other(
                "market pricing must define fixed_quote_per_base or \
                 min/max_price_quote_per_base for offer build"
                    .to_string(),
            )),
        }
    }

    #[must_use]
    pub fn base_mojo_multiplier(&self, asset_id: &str) -> i64 {
        self.base_unit_mojo_multiplier
            .unwrap_or_else(|| default_mojo_multiplier_for_asset(asset_id))
    }

    #[must_use]
    pub fn quote_mojo_multiplier(&self, asset_id: &str) -> i64 {
        self.quote_unit_mojo_multiplier
            .unwrap_or_else(|| default_mojo_multiplier_for_asset(asset_id))
    }

    #[must_use]
    pub fn mojo_multiplier_for_leg(&self, field: &str, asset_id: &str) -> i64 {
        match field {
            "quote_unit_mojo_multiplier" => self.quote_mojo_multiplier(asset_id),
            _ => self.base_mojo_multiplier(asset_id),
        }
    }
}

pub(super) struct ParsedPricing {
    pub pricing: MarketPricing,
    pub cancel_move_threshold_bps: Option<i64>,
}

/// Parse and validate market pricing; extract cancel threshold in one pass.
pub(super) fn parse_market_pricing(
    raw: Option<&Value>,
    base_asset: &str,
    quote_asset: &str,
    quote_asset_type: &str,
    market_id: &str,
) -> SignerResult<ParsedPricing> {
    let empty = Map::new();
    let obj = match raw {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(map)) => map,
        Some(_) => return Err(market_err(market_id, "pricing must be a mapping")),
    };

    reject_legacy_fields(obj, market_id)?;

    let cancel_move_threshold_bps =
        optional_positive_i64(obj, market_id, "cancel_move_threshold_bps")?;
    let strategy_target_spread_bps =
        optional_positive_i64(obj, market_id, "strategy_target_spread_bps")?;
    if strategy_target_spread_bps.is_some_and(|bps| bps >= 20_000) {
        return Err(market_err(
            market_id,
            "strategy_target_spread_bps must be < 20000",
        ));
    }
    let strategy_min_xch_price_usd =
        optional_positive_f64(obj, market_id, "strategy_min_xch_price_usd")?;
    let strategy_max_xch_price_usd =
        optional_positive_f64(obj, market_id, "strategy_max_xch_price_usd")?;
    if let (Some(min_price), Some(max_price)) =
        (strategy_min_xch_price_usd, strategy_max_xch_price_usd)
    {
        if min_price > max_price {
            return Err(market_err(
                market_id,
                "strategy_min_xch_price_usd must be <= strategy_max_xch_price_usd",
            ));
        }
    }

    let strategy_offer_expiry_minutes =
        optional_positive_i64(obj, market_id, "strategy_offer_expiry_minutes")?;
    warn_unstable_expiry(market_id, quote_asset_type, strategy_offer_expiry_minutes);

    Ok(ParsedPricing {
        pricing: MarketPricing {
            base_unit_mojo_multiplier: Some(canonicalize_multiplier(
                base_asset,
                obj.get("base_unit_mojo_multiplier"),
                "base_unit_mojo_multiplier",
                market_id,
            )?),
            quote_unit_mojo_multiplier: Some(canonicalize_multiplier(
                quote_asset,
                obj.get("quote_unit_mojo_multiplier"),
                "quote_unit_mojo_multiplier",
                market_id,
            )?),
            cancel_policy_stable_vs_unstable: obj
                .get("cancel_policy_stable_vs_unstable")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            strategy_target_spread_bps,
            strategy_min_xch_price_usd,
            strategy_max_xch_price_usd,
            strategy_offer_expiry_minutes,
            fixed_quote_per_base: optional_f64(obj, market_id, "fixed_quote_per_base")?,
            min_price_quote_per_base: optional_f64(obj, market_id, "min_price_quote_per_base")?,
            max_price_quote_per_base: optional_f64(obj, market_id, "max_price_quote_per_base")?,
            side: obj
                .get("side")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        },
        cancel_move_threshold_bps,
    })
}

/// Full spread in bps, split equally around mid. Buy below mid; sell above.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub(super) fn spread_adjusted_quote_price(mid: f64, spread_bps: Option<i64>, side: &str) -> f64 {
    let Some(bps) = spread_bps.filter(|value| *value > 0) else {
        return mid;
    };
    let half = (bps as f64) / 20_000.0;
    if side.trim().eq_ignore_ascii_case("buy") {
        mid * (1.0 - half)
    } else {
        mid * (1.0 + half)
    }
}

fn reject_legacy_fields(obj: &Map<String, Value>, market_id: &str) -> SignerResult<()> {
    for legacy_field in ["reference_source", "reference_pair"] {
        if obj.contains_key(legacy_field) {
            return Err(market_err(
                market_id,
                format!("{legacy_field} is no longer supported"),
            ));
        }
    }
    if obj.contains_key("strategy_offer_expiry_unit")
        || obj.contains_key("strategy_offer_expiry_value")
    {
        return Err(market_err(
            market_id,
            "strategy_offer_expiry_unit/value are no longer supported; use strategy_offer_expiry_minutes",
        ));
    }
    Ok(())
}

fn warn_unstable_expiry(market_id: &str, quote_asset_type: &str, expiry_minutes: Option<i64>) {
    let Some(expiry_minutes) = expiry_minutes else {
        return;
    };
    if quote_asset_type.trim().eq_ignore_ascii_case("unstable") && expiry_minutes > 15 {
        crate::trace_event!(
            WARN,
            LogContext::VALIDATION,
            MARKET_VALIDATION_WARNING,
            {
                market_id = market_id,
                field = "strategy_offer_expiry_minutes",
                value = expiry_minutes,
            };
            "unstable strategy_offer_expiry_minutes exceeds 15 minutes"
        );
    }
}

fn uses_cat_units(asset_id: &str) -> bool {
    let normalized = asset_id.trim().to_ascii_lowercase();
    !normalized.is_empty() && !matches!(normalized.as_str(), "xch" | "txch" | "1")
}

fn canonicalize_multiplier(
    asset_id: &str,
    raw_value: Option<&Value>,
    field_name: &str,
    market_id: &str,
) -> SignerResult<i64> {
    let default = if uses_cat_units(asset_id) {
        CANONICAL_CAT_UNIT_MOJOS
    } else {
        CANONICAL_XCH_UNIT_MOJOS
    };
    let Some(raw) = raw_value else {
        return Ok(default);
    };
    if matches!(raw, Value::Null) || raw.as_str().is_some_and(|text| text.trim().is_empty()) {
        return Ok(default);
    }
    let multiplier = market_i64(raw, market_id, field_name)?;
    if multiplier <= 0 {
        return Err(market_err(
            market_id,
            format!("{field_name} must be positive"),
        ));
    }
    if uses_cat_units(asset_id) && multiplier != CANONICAL_CAT_UNIT_MOJOS {
        return Err(market_err(
            market_id,
            format!("{field_name} must be 1000 for CAT assets"),
        ));
    }
    Ok(multiplier)
}

fn optional_positive_i64(
    obj: &Map<String, Value>,
    market_id: &str,
    field: &str,
) -> SignerResult<Option<i64>> {
    let Some(raw) = obj.get(field) else {
        return Ok(None);
    };
    let value = market_i64(raw, market_id, field)?;
    if value <= 0 {
        return Err(market_err(market_id, format!("{field} must be positive")));
    }
    Ok(Some(value))
}

fn optional_positive_f64(
    obj: &Map<String, Value>,
    market_id: &str,
    field: &str,
) -> SignerResult<Option<f64>> {
    let Some(raw) = obj.get(field) else {
        return Ok(None);
    };
    let value = market_f64(raw, market_id, field)?;
    if value <= 0.0 {
        return Err(market_err(market_id, format!("{field} must be > 0")));
    }
    Ok(Some(value))
}

fn optional_f64(
    obj: &Map<String, Value>,
    market_id: &str,
    field: &str,
) -> SignerResult<Option<f64>> {
    let Some(raw) = obj.get(field) else {
        return Ok(None);
    };
    Ok(Some(market_f64(raw, market_id, field)?))
}

fn market_err(market_id: &str, message: impl AsRef<str>) -> SignerError {
    SignerError::Other(format!("market {market_id}: {}", message.as_ref()))
}

fn market_i64(raw: &Value, market_id: &str, field: &str) -> SignerResult<i64> {
    parse_i64_field(raw, &format!("market {market_id}: {field}"))
}

fn market_f64(raw: &Value, market_id: &str, field: &str) -> SignerResult<f64> {
    parse_f64_field(raw, &format!("market {market_id}: {field}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operator_log::{TraceCapture, MARKET_VALIDATION_WARNING};
    use serde_json::json;

    #[test]
    fn unstable_expiry_above_15_minutes_passes_and_warns() {
        let capture = TraceCapture::install();
        let parsed = parse_market_pricing(
            Some(&json!({"strategy_offer_expiry_minutes": 30})),
            "BYC",
            "xch",
            "unstable",
            "m1",
        )
        .expect("valid");
        assert_eq!(parsed.pricing.strategy_offer_expiry_minutes, Some(30));
        assert_eq!(capture.count_substr(MARKET_VALIDATION_WARNING), 1);
    }

    #[test]
    fn stable_expiry_above_15_minutes_is_allowed() {
        let parsed = parse_market_pricing(
            Some(&json!({"strategy_offer_expiry_minutes": 60})),
            "BYC",
            "xch",
            "stable",
            "m1",
        )
        .expect("valid");
        assert_eq!(parsed.pricing.strategy_offer_expiry_minutes, Some(60));
    }

    #[test]
    fn extracts_cancel_threshold_without_keeping_it_on_pricing() {
        let parsed = parse_market_pricing(
            Some(&json!({"cancel_move_threshold_bps": 250})),
            "BYC",
            "xch",
            "unstable",
            "m1",
        )
        .expect("valid");
        assert_eq!(parsed.cancel_move_threshold_bps, Some(250));
    }

    #[test]
    fn spread_adjusted_quote_price_offsets_half_spread_around_mid() {
        let buy = spread_adjusted_quote_price(1.0, Some(20), "buy");
        let sell = spread_adjusted_quote_price(1.0, Some(20), "sell");
        assert!((buy - 0.999).abs() < 1e-12);
        assert!((sell - 1.001).abs() < 1e-12);
        assert!(buy < 1.0 && 1.0 < sell);
    }

    #[test]
    fn spread_adjusted_quote_price_without_spread_is_mid() {
        assert!((spread_adjusted_quote_price(0.999, None, "buy") - 0.999).abs() < 1e-12);
        assert!((spread_adjusted_quote_price(0.999, None, "sell") - 0.999).abs() < 1e-12);
    }

    #[test]
    fn parse_rejects_spread_that_would_zero_the_buy_side() {
        let err = parse_market_pricing(
            Some(&json!({"strategy_target_spread_bps": 20_000})),
            "BYC",
            "wUSDC.b",
            "stable",
            "m1",
        )
        .err()
        .expect("spread too wide");
        assert!(err.to_string().contains("strategy_target_spread_bps"));
    }
}
