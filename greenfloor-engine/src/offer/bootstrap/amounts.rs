//! Bootstrap ladder amounts (base units) and conversion to on-chain mojos at vault submit.

use crate::coin_ops::coin_op_non_negative_u64;
use crate::error::SignerResult;

/// Ladder/base-unit amount used only in bootstrap planning (`1 CAT unit = 1_000 mojos` when multiplier is `1_000`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BaseUnits(pub i64);

/// On-chain mojos for vault mixed-split I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mojos(pub i64);

impl BaseUnits {
    #[must_use]
    pub fn to_mojos(self, mojo_multiplier: i64) -> Mojos {
        Mojos(self.0.saturating_mul(mojo_multiplier.max(1)))
    }
}

#[must_use]
pub fn base_units_to_mojos(base_units: i64, mojo_multiplier: i64) -> i64 {
    BaseUnits(base_units).to_mojos(mojo_multiplier).0
}

/// Change mojos after combining bootstrap inputs into a target-sized coin.
#[must_use]
pub fn bootstrap_overshoot_change_mojos(
    selected_total_base_units: i64,
    target_amount_base_units: i64,
    mojo_multiplier: i64,
) -> i64 {
    BaseUnits(selected_total_base_units.saturating_sub(target_amount_base_units))
        .to_mojos(mojo_multiplier)
        .0
}

/// Convert a bootstrap mixed-split output list to vault output amounts (mojos).
///
/// # Errors
///
/// Returns an error when a converted amount is negative.
pub fn bootstrap_mixed_split_output_mojos(
    output_amounts_base_units: &[i64],
    mojo_multiplier: i64,
) -> SignerResult<Vec<u64>> {
    output_amounts_base_units
        .iter()
        .map(|amount| {
            coin_op_non_negative_u64(
                base_units_to_mojos(*amount, mojo_multiplier),
                "bootstrap.output_amount_mojos",
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_units_to_mojos_scales_cat_amounts() {
        assert_eq!(base_units_to_mojos(100, 1_000), 100_000);
        assert_eq!(base_units_to_mojos(5, 1_000), 5_000);
    }

    #[test]
    fn bootstrap_overshoot_change_uses_mojo_multiplier() {
        assert_eq!(bootstrap_overshoot_change_mojos(105, 100, 1_000), 5_000);
    }
}
