//! Config and runtime scalar integer conversions.
//!
//! Policy: config and runtime scalar reads propagate errors via `config_err` (no silent fallback).
//! Float casts for YAML fields use `offer::pricing`.

use crate::config::yaml_fields::config_err;
use crate::error::SignerResult;

/// Parse non negative u64.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn parse_non_negative_u64(value: i64, field: &str) -> SignerResult<u64> {
    if value < 0 {
        return Err(config_err(format!("{field} must be >= 0")));
    }
    u64::try_from(value).map_err(|_| config_err(format!("{field} must fit in u64")))
}

/// U64 to i64.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn u64_to_i64(value: u64, field: &str) -> SignerResult<i64> {
    i64::try_from(value).map_err(|_| config_err(format!("{field} must fit in i64")))
}

/// Usize to i64.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn usize_to_i64(value: usize, field: &str) -> SignerResult<i64> {
    i64::try_from(value).map_err(|_| config_err(format!("{field} must fit in i64")))
}
