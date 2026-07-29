//! Bootstrap ladder amounts (base units) and conversion to on-chain mojos at vault submit.

use crate::coin_ops::{
    coin_op_non_negative_u64, combine_output_amounts, COMBINE_SINGLE_OUTPUT_COUNT,
};
use crate::error::SignerResult;

/// Ladder/base-unit amount used only in bootstrap planning (`1 CAT unit = 1_000 mojos` when multiplier is `1_000`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BaseUnits(pub i64);

/// Selected inputs for bootstrap combine-first (base units only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapCombineInputs {
    pub input_coin_ids: Vec<String>,
    pub selected_total: BaseUnits,
    pub target_amount: BaseUnits,
    pub exact_match: bool,
    pub cap_applied: bool,
}

/// On-chain mojos for vault mixed-split I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Mojos(pub i64);

impl BaseUnits {
    #[must_use]
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }

    #[must_use]
    pub fn to_mojos(self, mojo_multiplier: i64) -> Mojos {
        Mojos(self.0.saturating_mul(mojo_multiplier.max(1)))
    }
}

#[must_use]
pub fn base_units_to_mojos(base_units: BaseUnits, mojo_multiplier: i64) -> i64 {
    base_units.to_mojos(mojo_multiplier).0
}

/// Change mojos after combining bootstrap inputs into a target-sized coin.
#[must_use]
pub fn bootstrap_overshoot_change_mojos(
    selected_total: BaseUnits,
    target_amount: BaseUnits,
    mojo_multiplier: i64,
) -> i64 {
    BaseUnits(selected_total.0.saturating_sub(target_amount.0))
        .to_mojos(mojo_multiplier)
        .0
}

/// Convert a bootstrap mixed-split output list to vault output amounts (mojos).
///
/// # Errors
///
/// Returns an error when a converted amount is negative.
pub fn bootstrap_mixed_split_output_mojos(
    output_amounts_base_units: &[BaseUnits],
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

/// Single combine output in mojos for bootstrap combine-first (`target_amount` base units).
///
/// # Errors
///
/// Returns an error when output encoding fails.
pub(crate) fn bootstrap_combine_vault_outputs(
    inputs: &BootstrapCombineInputs,
    mojo_multiplier: i64,
) -> SignerResult<Vec<u64>> {
    let output_mojos = base_units_to_mojos(inputs.target_amount, mojo_multiplier);
    combine_output_amounts(output_mojos, COMBINE_SINGLE_OUTPUT_COUNT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_units_to_mojos_scales_cat_amounts() {
        assert_eq!(base_units_to_mojos(BaseUnits::new(100), 1_000), 100_000);
        assert_eq!(base_units_to_mojos(BaseUnits::new(5), 1_000), 5_000);
    }

    #[test]
    fn bootstrap_overshoot_change_uses_mojo_multiplier() {
        assert_eq!(
            bootstrap_overshoot_change_mojos(BaseUnits::new(105), BaseUnits::new(100), 1_000),
            5_000
        );
    }

    #[test]
    fn eco181_shape_outputs_target_not_selected_total() {
        let inputs = BootstrapCombineInputs {
            input_coin_ids: vec!["a".repeat(64), "b".repeat(64)],
            selected_total: BaseUnits::new(105),
            target_amount: BaseUnits::new(100),
            exact_match: false,
            cap_applied: true,
        };
        let outputs = bootstrap_combine_vault_outputs(&inputs, 1_000).expect("outputs");
        assert_eq!(outputs, vec![100_000]);
    }
}
