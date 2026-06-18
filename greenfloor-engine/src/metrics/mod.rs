//! Saturating numeric conversions for daemon/cycle aggregation metrics.
//!
//! Policy: metric rollups are best-effort totals. Negative per-market counters clamp to zero;
//! lengths and counters above `u64::MAX` clamp to `u64::MAX` (documented saturation, not errors).

/// Convert a collection length for metric counters.
#[must_use]
pub fn collection_len_to_u64(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

/// Convert a collection length for cycle state counters (`i64` fields).
#[must_use]
pub fn collection_len_to_i64(len: usize) -> i64 {
    i64::try_from(len).unwrap_or(i64::MAX)
}

/// Saturating conversion for elapsed-millis timing fields (overflow → `u64::MAX`).
#[must_use]
pub fn millis_to_u64(ms: u128) -> u64 {
    u64::try_from(ms).unwrap_or(u64::MAX)
}

/// Convert a possibly-negative cycle counter for metric aggregation.
#[must_use]
pub fn non_negative_i64_to_u64(value: i64) -> u64 {
    u64::try_from(value.max(0)).unwrap_or(u64::MAX)
}
