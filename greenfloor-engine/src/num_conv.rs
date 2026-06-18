//! Numeric conversions with explicit error handling for clippy-friendly casts.

use crate::error::{SignerError, SignerResult};

pub fn i64_to_u64(value: i64) -> SignerResult<u64> {
    u64::try_from(value).map_err(|_| SignerError::InvalidOutputAmount)
}

pub fn u64_to_i64(value: u64) -> SignerResult<i64> {
    i64::try_from(value).map_err(|_| SignerError::InvalidOutputAmount)
}

pub fn usize_to_i64(value: usize) -> SignerResult<i64> {
    i64::try_from(value).map_err(|_| SignerError::Other("value out of i64 range".into()))
}

pub fn i64_to_usize(value: i64) -> SignerResult<usize> {
    if value < 0 {
        return Err(SignerError::InvalidOutputAmount);
    }
    usize::try_from(value).map_err(|_| SignerError::InvalidOutputAmount)
}

pub fn u64_to_usize(value: u64) -> SignerResult<usize> {
    usize::try_from(value).map_err(|_| SignerError::InvalidOutputAmount)
}

pub fn u128_to_u64(value: u128) -> SignerResult<u64> {
    u64::try_from(value).map_err(|_| SignerError::InvalidOutputAmount)
}

pub fn u32_to_i32(value: u32) -> SignerResult<i32> {
    i32::try_from(value).map_err(|_| SignerError::Other("value out of i32 range".into()))
}

/// Convert u64 to f64 for price/ratio calculations (~52-bit mantissa is acceptable).
#[allow(clippy::cast_precision_loss)]
#[must_use]
pub fn u64_to_f64(value: u64) -> f64 {
    value as f64
}

/// Convert i64 to f64 for price/ratio calculations (~52-bit mantissa is acceptable).
#[allow(clippy::cast_precision_loss)]
#[must_use]
pub fn i64_to_f64(value: i64) -> f64 {
    value as f64
}

/// Round an f64 to i64 after price math; rejects non-finite or out-of-range values.
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn f64_to_i64_round(value: f64) -> SignerResult<i64> {
    let rounded = value.round();
    if !rounded.is_finite() {
        return Err(SignerError::InvalidOutputAmount);
    }
    if rounded < i64_to_f64(i64::MIN) || rounded > i64_to_f64(i64::MAX) {
        return Err(SignerError::InvalidOutputAmount);
    }
    Ok(rounded as i64)
}

/// Quote-leg mojos for a base size at the given price and unit multiplier.
pub fn quote_mojos_for_base_size(
    size_base_units: i64,
    quote_price: f64,
    quote_unit_multiplier: i64,
) -> SignerResult<i64> {
    f64_to_i64_round(i64_to_f64(size_base_units) * quote_price * i64_to_f64(quote_unit_multiplier))
}

/// Ladder combine threshold: `ceil(target_count * factor)` with a minimum of 2.
pub fn combine_threshold_count(
    target_count: i64,
    combine_when_excess_factor: f64,
) -> SignerResult<i64> {
    f64_to_i64_round(
        (i64_to_f64(target_count) * combine_when_excess_factor)
            .ceil()
            .max(2.0),
    )
}
