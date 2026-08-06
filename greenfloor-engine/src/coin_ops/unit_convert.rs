//! Shared CAT / ladder unit vocabulary.
//!
//! - **On-chain / planning math:** mojos.
//! - **Config ladder sizes:** whole CAT units (`markets.yaml` `size_base_units`).
//! - **Display:** CAT units, including fractions (`10.5` CAT = `10_500` mojos).
//!
//! A fractional coin is valid inventory. It is **not** an exact ladder clip: bucket matching
//! and cannibalization protection only treat `amount % multiplier == 0` as a whole-unit size.

use std::str::FromStr;

use crate::hex::CANONICAL_CAT_MOJOS;

/// Mojos per one CAT config/display unit (`1000`). Prefer this over raw `1000` literals.
pub const CAT_MOJOS_PER_UNIT: i64 = CANONICAL_CAT_MOJOS;

/// Convert whole ladder/config units to on-chain mojos.
#[must_use]
pub fn mojos_from_whole_units(whole_units: i64, mojo_multiplier: i64) -> i64 {
    whole_units.saturating_mul(mojo_multiplier.max(1))
}

/// Whole ladder/config units only when `amount_mojos` is an exact multiple of the multiplier.
///
/// `10_500` mojos with multiplier `1000` → `None` (valid `10.5` CAT coin, not size `10`).
#[must_use]
pub fn exact_whole_units_from_mojos(amount_mojos: i64, mojo_multiplier: i64) -> Option<i64> {
    let multiplier = mojo_multiplier.max(1);
    if amount_mojos <= 0 || amount_mojos % multiplier != 0 {
        return None;
    }
    Some(amount_mojos / multiplier)
}

/// Format mojos as a CAT-units decimal string (`10500` → `"10.5"`).
#[must_use]
pub fn cat_units_string_from_mojos(mojos: u64, mojos_per_unit: i64) -> String {
    let divisor = u64::try_from(mojos_per_unit.max(1)).unwrap_or(1);
    let whole = mojos / divisor;
    let frac = mojos % divisor;
    if frac == 0 {
        return whole.to_string();
    }
    let width = usize::try_from(divisor.ilog10()).unwrap_or(0);
    let frac_text = format!("{frac:0width$}");
    let frac_text = frac_text.trim_end_matches('0');
    format!("{whole}.{frac_text}")
}

/// CAT display units from mojos, preserving fractions (`239950` → `239.95`).
#[must_use]
pub fn cat_units_display_from_mojos(mojos: u64) -> serde_json::Value {
    cat_units_display_from_mojos_with_multiplier(mojos, CAT_MOJOS_PER_UNIT)
}

/// CAT (or other) display units from mojos using an explicit per-unit multiplier.
#[must_use]
pub fn cat_units_display_from_mojos_with_multiplier(
    mojos: u64,
    mojos_per_unit: i64,
) -> serde_json::Value {
    let text = cat_units_string_from_mojos(mojos, mojos_per_unit);
    match serde_json::Number::from_str(&text) {
        Ok(number) => serde_json::Value::Number(number),
        Err(_) => serde_json::Value::String(text),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        cat_units_display_from_mojos, cat_units_string_from_mojos, exact_whole_units_from_mojos,
        mojos_from_whole_units, CAT_MOJOS_PER_UNIT,
    };

    #[test]
    fn exact_whole_units_requires_clean_multiple() {
        assert_eq!(exact_whole_units_from_mojos(10_000, 1_000), Some(10));
        assert_eq!(exact_whole_units_from_mojos(10_500, 1_000), None);
        assert_eq!(exact_whole_units_from_mojos(10_010, 1_000), None);
        assert_eq!(exact_whole_units_from_mojos(0, 1_000), None);
        assert_eq!(exact_whole_units_from_mojos(5_000, 1), Some(5_000));
    }

    #[test]
    fn mojos_from_whole_units_scales() {
        assert_eq!(mojos_from_whole_units(10, CAT_MOJOS_PER_UNIT), 10_000);
        assert_eq!(mojos_from_whole_units(10, 1), 10);
    }

    #[test]
    fn cat_units_display_preserves_fractional_coins() {
        assert_eq!(
            cat_units_string_from_mojos(10_500, CAT_MOJOS_PER_UNIT),
            "10.5"
        );
        assert_eq!(
            cat_units_string_from_mojos(239_950, CAT_MOJOS_PER_UNIT),
            "239.95"
        );
        assert_eq!(cat_units_display_from_mojos(10_500).to_string(), "10.5");
        assert_eq!(cat_units_display_from_mojos(239_950).to_string(), "239.95");
        assert_eq!(cat_units_display_from_mojos(1_000).to_string(), "1");
        assert_eq!(cat_units_display_from_mojos(100).to_string(), "0.1");
        assert_eq!(cat_units_display_from_mojos(0).to_string(), "0");
    }
}
