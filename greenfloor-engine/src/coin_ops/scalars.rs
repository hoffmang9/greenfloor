//! Coin-op scalar conversions.
//!
//! Policy: validated plan/CLI inputs propagate errors (`InvalidPlanValues`). Output amount
//! vectors use saturating clamps when splitting totals (overflow → `u64::MAX`).

use crate::error::{SignerError, SignerResult};

pub fn non_negative_i64_to_u64(value: i64, field: &str) -> SignerResult<u64> {
    if value < 0 {
        return Err(SignerError::InvalidPlanValues);
    }
    u64::try_from(value)
        .map_err(|_| SignerError::Other(format!("{field} must fit in u64 for coin-op execution")))
}

pub fn i64_to_usize(value: i64, field: &str) -> SignerResult<usize> {
    if value < 0 {
        return Err(SignerError::InvalidPlanValues);
    }
    usize::try_from(value)
        .map_err(|_| SignerError::Other(format!("{field} must fit in usize for coin-op execution")))
}

/// Saturating clamp for combine/split output leg amounts (validated totals; overflow → max).
#[must_use]
pub fn non_negative_i64_to_u64_saturating(value: i64) -> u64 {
    u64::try_from(value.max(0)).unwrap_or(u64::MAX)
}
