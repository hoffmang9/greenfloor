use log::warn;
use serde_json::Value;

use crate::error::{SignerError, SignerResult};

const CANONICAL_CAT_UNIT_MOJOS: i64 = 1000;
const CANONICAL_XCH_UNIT_MOJOS: i64 = 1_000_000_000_000;

fn uses_cat_units(asset_id: &str) -> bool {
    let normalized = asset_id.trim().to_ascii_lowercase();
    !normalized.is_empty() && !matches!(normalized.as_str(), "xch" | "txch" | "1")
}

fn market_err(market_id: &str, message: impl AsRef<str>) -> SignerError {
    SignerError::Other(format!("market {market_id}: {}", message.as_ref()))
}

fn parse_i64_field(raw: &Value, market_id: &str, field: &str) -> SignerResult<i64> {
    if let Some(value) = raw.as_i64() {
        return Ok(value);
    }
    if let Some(value) = raw.as_u64() {
        return Ok(value as i64);
    }
    if let Some(text) = raw.as_str() {
        if let Ok(value) = text.parse::<i64>() {
            return Ok(value);
        }
    }
    Err(market_err(market_id, format!("{field} must be an integer")))
}

fn parse_f64_field(raw: &Value, market_id: &str, field: &str) -> SignerResult<f64> {
    if let Some(value) = raw.as_f64() {
        return Ok(value);
    }
    if let Some(value) = raw.as_i64() {
        return Ok(value as f64);
    }
    if let Some(value) = raw.as_u64() {
        return Ok(value as f64);
    }
    if let Some(text) = raw.as_str() {
        if let Ok(value) = text.parse::<f64>() {
            return Ok(value);
        }
    }
    Err(market_err(market_id, format!("{field} must be numeric")))
}

fn is_missing_multiplier(raw: Option<&Value>) -> bool {
    match raw {
        None | Some(Value::Null) => true,
        Some(Value::String(text)) => text.trim().is_empty(),
        _ => false,
    }
}

pub fn canonicalize_asset_unit_mojo_multiplier(
    asset_id: &str,
    raw_value: Option<&Value>,
    field_name: &str,
    market_id: &str,
) -> SignerResult<i64> {
    if is_missing_multiplier(raw_value) {
        return Ok(if uses_cat_units(asset_id) {
            CANONICAL_CAT_UNIT_MOJOS
        } else {
            CANONICAL_XCH_UNIT_MOJOS
        });
    }

    let raw = raw_value.expect("checked above");
    let multiplier = parse_i64_field(raw, market_id, field_name)?;
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

pub fn validate_strategy_pricing(
    pricing: &Value,
    market_id: &str,
    quote_asset_type: &str,
) -> SignerResult<()> {
    let quote_type = quote_asset_type.trim().to_ascii_lowercase();
    let pricing_obj = pricing.as_object().ok_or_else(|| {
        SignerError::Other(format!("market {market_id}: pricing must be a mapping"))
    })?;

    for legacy_field in ["reference_source", "reference_pair"] {
        if pricing_obj.get(legacy_field).is_some() {
            return Err(market_err(
                market_id,
                format!("{legacy_field} is no longer supported"),
            ));
        }
    }

    if let Some(spread_raw) = pricing_obj.get("strategy_target_spread_bps") {
        let spread = parse_i64_field(spread_raw, market_id, "strategy_target_spread_bps")?;
        if spread <= 0 {
            return Err(market_err(
                market_id,
                "strategy_target_spread_bps must be positive",
            ));
        }
    }

    let mut min_price: Option<f64> = None;
    let mut max_price: Option<f64> = None;
    if let Some(min_raw) = pricing_obj.get("strategy_min_xch_price_usd") {
        let parsed = parse_f64_field(min_raw, market_id, "strategy_min_xch_price_usd")?;
        if parsed <= 0.0 {
            return Err(market_err(
                market_id,
                "strategy_min_xch_price_usd must be > 0",
            ));
        }
        min_price = Some(parsed);
    }
    if let Some(max_raw) = pricing_obj.get("strategy_max_xch_price_usd") {
        let parsed = parse_f64_field(max_raw, market_id, "strategy_max_xch_price_usd")?;
        if parsed <= 0.0 {
            return Err(market_err(
                market_id,
                "strategy_max_xch_price_usd must be > 0",
            ));
        }
        max_price = Some(parsed);
    }
    if let (Some(min_price), Some(max_price)) = (min_price, max_price) {
        if min_price > max_price {
            return Err(market_err(
                market_id,
                "strategy_min_xch_price_usd must be <= strategy_max_xch_price_usd",
            ));
        }
    }

    if pricing_obj.contains_key("strategy_offer_expiry_unit")
        || pricing_obj.contains_key("strategy_offer_expiry_value")
    {
        return Err(market_err(
            market_id,
            "strategy_offer_expiry_unit/value are no longer supported; use strategy_offer_expiry_minutes",
        ));
    }

    if let Some(expiry_raw) = pricing_obj.get("strategy_offer_expiry_minutes") {
        let expiry_minutes =
            parse_i64_field(expiry_raw, market_id, "strategy_offer_expiry_minutes")?;
        if expiry_minutes <= 0 {
            return Err(market_err(
                market_id,
                "strategy_offer_expiry_minutes must be positive",
            ));
        }
        if quote_type == "unstable" && expiry_minutes > 15 {
            warn!(
                "market {market_id}: unstable strategy_offer_expiry_minutes={expiry_minutes} exceeds 15 minutes"
            );
        }
    }

    if let Some(threshold_raw) = pricing_obj.get("cancel_move_threshold_bps") {
        let threshold = parse_i64_field(threshold_raw, market_id, "cancel_move_threshold_bps")?;
        if threshold <= 0 {
            return Err(market_err(
                market_id,
                "cancel_move_threshold_bps must be positive",
            ));
        }
    }

    Ok(())
}

pub fn pop_cancel_move_threshold_bps(pricing: &mut Value) -> Option<i64> {
    let pricing_obj = pricing.as_object_mut()?;
    let raw = pricing_obj.remove("cancel_move_threshold_bps")?;
    raw.as_i64()
        .or_else(|| raw.as_u64().map(|value| value as i64))
}
