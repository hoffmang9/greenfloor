//! Coin-op scalar conversions.
//!
//! Policy: validated plan/CLI inputs propagate errors (`InvalidPlanValues`). Output amount
//! vectors use `coin_op_non_negative_u64_saturating` when splitting totals (overflow → `u64::MAX`).

use crate::error::{SignerError, SignerResult};

/// Coin op non negative u64.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn coin_op_non_negative_u64(value: i64, field: &str) -> SignerResult<u64> {
    if value < 0 {
        return Err(SignerError::InvalidPlanValues);
    }
    u64::try_from(value)
        .map_err(|_| SignerError::Other(format!("{field} must fit in u64 for coin-op execution")))
}

/// I64 to usize.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn i64_to_usize(value: i64, field: &str) -> SignerResult<usize> {
    if value < 0 {
        return Err(SignerError::InvalidPlanValues);
    }
    usize::try_from(value)
        .map_err(|_| SignerError::Other(format!("{field} must fit in usize for coin-op execution")))
}

/// Usize to i64.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn usize_to_i64(value: usize, field: &str) -> SignerResult<i64> {
    i64::try_from(value)
        .map_err(|_| SignerError::Other(format!("{field} must fit in i64 for coin-op execution")))
}

/// Saturating clamp for combine/split output leg amounts (validated totals; overflow → max).
#[must_use]
pub fn coin_op_non_negative_u64_saturating(value: i64) -> u64 {
    u64::try_from(value.max(0)).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SignerError;

    #[test]
    fn coin_op_non_negative_u64_rejects_negative_and_overflow() {
        assert_eq!(coin_op_non_negative_u64(10, "amount").expect("ok"), 10);
        assert!(matches!(
            coin_op_non_negative_u64(-1, "amount"),
            Err(SignerError::InvalidPlanValues)
        ));
    }

    #[test]
    fn i64_to_usize_and_usize_to_i64_convert_valid_values() {
        assert_eq!(i64_to_usize(4, "count").expect("usize"), 4);
        assert_eq!(usize_to_i64(4, "count").expect("i64"), 4);
        assert!(i64_to_usize(-1, "count").is_err());
    }

    #[test]
    fn coin_op_non_negative_u64_saturating_clamps() {
        assert_eq!(coin_op_non_negative_u64_saturating(-5), 0);
        assert_eq!(coin_op_non_negative_u64_saturating(42), 42);
    }
}
