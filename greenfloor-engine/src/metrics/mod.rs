//! Saturating numeric conversions for daemon/cycle aggregation metrics.
//!
//! Policy: metric rollups are best-effort totals — not operator correctness paths.
//! Negative per-market counters clamp to zero; lengths and counters above the target type
//! clamp to the type maximum (documented saturation, not errors).
//! For config reads use `config::parse_int` (`parse_non_negative_u64`, etc.);
//! for offer/ladder math use `offer::pricing`; for coin-op execution use `coin_ops::scalars`.

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
pub fn metric_non_negative_u64(value: i64) -> u64 {
    u64::try_from(value.max(0)).unwrap_or(u64::MAX)
}

/// Convert a non-negative runtime scalar for daemon dispatch (overflow → `usize::MAX`).
#[must_use]
pub fn non_negative_i64_to_usize(value: i64) -> usize {
    usize::try_from(value.max(0)).unwrap_or(usize::MAX)
}

/// Convert a non-negative runtime `u64` scalar for daemon dispatch (overflow → `usize::MAX`).
#[must_use]
pub fn non_negative_u64_to_usize(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}
