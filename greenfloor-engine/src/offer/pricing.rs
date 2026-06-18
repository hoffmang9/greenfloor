//! Offer and ladder price math using `f64` ratios (~52-bit mantissa is acceptable).
//!
//! Policy: non-finite or out-of-range ladder/offer math returns `SignerError` (no silent zero).
//! Offer-leg quote mojos use `InvalidOfferRequestAmount`; ladder thresholds use `InvalidLadderMath`.

use crate::error::{SignerError, SignerResult};

#[allow(clippy::cast_precision_loss)]
#[must_use]
pub fn i64_to_f64(value: i64) -> f64 {
    value as f64
}

#[allow(clippy::cast_precision_loss)]
#[must_use]
pub fn u64_to_f64(value: u64) -> f64 {
    value as f64
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn f64_to_i64_round_internal(value: f64) -> Result<i64, ()> {
    let rounded = value.round();
    if !rounded.is_finite() {
        return Err(());
    }
    if rounded < i64_to_f64(i64::MIN) || rounded > i64_to_f64(i64::MAX) {
        return Err(());
    }
    Ok(rounded as i64)
}

/// Round an `f64` to `i64` for ladder math; rejects non-finite or out-of-range values.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn f64_to_i64_round_ladder(value: f64) -> SignerResult<i64> {
    f64_to_i64_round_internal(value).map_err(|()| SignerError::InvalidLadderMath)
}

/// Quote-leg mojos for a base size at the given price and unit multiplier.
pub fn quote_mojos_for_base_size(
    size_base_units: i64,
    quote_price: f64,
    quote_unit_multiplier: i64,
) -> SignerResult<i64> {
    f64_to_i64_round_internal(
        i64_to_f64(size_base_units) * quote_price * i64_to_f64(quote_unit_multiplier),
    )
    .map_err(|()| SignerError::InvalidOfferRequestAmount)
}

/// Ladder combine threshold: `ceil(target_count * factor)` with a minimum of 2.
pub fn combine_threshold_count(
    target_count: i64,
    combine_when_excess_factor: f64,
) -> SignerResult<i64> {
    let scaled = i64_to_f64(target_count) * combine_when_excess_factor;
    if !scaled.is_finite() {
        return Err(SignerError::InvalidLadderMath);
    }
    f64_to_i64_round_ladder(scaled.ceil().max(2.0))
}
