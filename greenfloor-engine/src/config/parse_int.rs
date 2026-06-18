//! Config and runtime scalar integer conversions (propagate errors; no silent fallback).

use crate::config::yaml_fields::config_err;
use crate::error::SignerResult;

pub fn non_negative_i64_to_u64(value: i64, field: &str) -> SignerResult<u64> {
    if value < 0 {
        return Err(config_err(format!("{field} must be >= 0")));
    }
    u64::try_from(value).map_err(|_| config_err(format!("{field} must fit in u64")))
}

pub fn u64_to_i64(value: u64, field: &str) -> SignerResult<i64> {
    i64::try_from(value).map_err(|_| config_err(format!("{field} must fit in i64")))
}

pub fn usize_to_i64(value: usize, field: &str) -> SignerResult<i64> {
    i64::try_from(value).map_err(|_| config_err(format!("{field} must fit in i64")))
}

#[allow(clippy::cast_precision_loss)]
pub fn i64_to_f64(value: i64) -> f64 {
    value as f64
}

#[allow(clippy::cast_precision_loss)]
pub fn u64_to_f64(value: u64) -> f64 {
    value as f64
}
