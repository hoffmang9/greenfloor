//! Bootstrap plan amounts and conversion to on-chain mojos at vault submit.
//!
//! Signer denomination plans in mojos and submits with identity conversion
//! ([`vault_output_mojos_from_plan_amounts`] / [`bootstrap_combine_vault_outputs_as_mojos`]).
//! Scaled plan-unit → mojos helpers live behind `cfg(test)` for planner fixtures only.

use crate::coin_ops::{
    coin_op_non_negative_u64, combine_output_amounts, COMBINE_SINGLE_OUTPUT_COUNT,
};
use crate::error::SignerResult;

/// Amount used in bootstrap ladder/coin matching.
///
/// On the signer denomination path these values are mojos. Planner unit tests may use
/// other plan units paired with [`crate::coin_ops::shape::AmountUnit::PlanUnits`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PlanAmount(pub i64);

impl PlanAmount {
    #[must_use]
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// Convert plan amounts that are already mojos into vault mixed-split outputs.
///
/// # Errors
///
/// Returns an error when a converted amount is negative.
pub fn vault_output_mojos_from_plan_amounts(plan_amounts: &[PlanAmount]) -> SignerResult<Vec<u64>> {
    plan_amounts
        .iter()
        .map(|amount| coin_op_non_negative_u64(amount.get(), "bootstrap.output_amount_mojos"))
        .collect()
}

/// Single combine output when `inputs.target_amount` is already mojos.
///
/// # Errors
///
/// Returns an error when output encoding fails.
pub(crate) fn bootstrap_combine_vault_outputs_as_mojos(
    inputs: &crate::coin_ops::shape::CombineInputs,
) -> SignerResult<Vec<u64>> {
    combine_output_amounts(inputs.target_amount, COMBINE_SINGLE_OUTPUT_COUNT)
}

#[cfg(test)]
mod scaled {
    //! Test-only plan-unit × multiplier → mojos conversion for planner fixtures.
    use super::PlanAmount;
    use crate::coin_ops::{
        coin_op_non_negative_u64, combine_output_amounts, COMBINE_SINGLE_OUTPUT_COUNT,
    };
    use crate::error::SignerResult;

    #[must_use]
    pub fn plan_amount_to_mojos(amount: PlanAmount, mojo_multiplier: i64) -> i64 {
        amount.0.saturating_mul(mojo_multiplier.max(1))
    }

    #[must_use]
    pub fn bootstrap_overshoot_change_mojos(
        selected_total: PlanAmount,
        target_amount: PlanAmount,
        mojo_multiplier: i64,
    ) -> i64 {
        plan_amount_to_mojos(
            PlanAmount(selected_total.0.saturating_sub(target_amount.0)),
            mojo_multiplier,
        )
    }

    /// Convert scaled plan amounts to vault output mojos (`plan * mojo_multiplier`).
    ///
    /// # Errors
    ///
    /// Returns an error when a converted amount is negative.
    pub fn bootstrap_mixed_split_output_mojos(
        output_amounts: &[PlanAmount],
        mojo_multiplier: i64,
    ) -> SignerResult<Vec<u64>> {
        output_amounts
            .iter()
            .map(|amount| {
                coin_op_non_negative_u64(
                    plan_amount_to_mojos(*amount, mojo_multiplier),
                    "bootstrap.output_amount_mojos",
                )
            })
            .collect()
    }

    /// Single combine output in mojos for scaled plan units (`target_amount * mojo_multiplier`).
    ///
    /// # Errors
    ///
    /// Returns an error when output encoding fails.
    pub fn bootstrap_combine_vault_outputs(
        inputs: &crate::coin_ops::shape::CombineInputs,
        mojo_multiplier: i64,
    ) -> SignerResult<Vec<u64>> {
        let output_mojos =
            plan_amount_to_mojos(PlanAmount::new(inputs.target_amount), mojo_multiplier);
        combine_output_amounts(output_mojos, COMBINE_SINGLE_OUTPUT_COUNT)
    }
}

#[cfg(test)]
pub use scaled::{
    bootstrap_combine_vault_outputs, bootstrap_mixed_split_output_mojos,
    bootstrap_overshoot_change_mojos, plan_amount_to_mojos,
};

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(test)]
    use scaled::{
        bootstrap_combine_vault_outputs, bootstrap_overshoot_change_mojos, plan_amount_to_mojos,
    };

    #[test]
    fn plan_amount_to_mojos_scales_cat_amounts() {
        assert_eq!(plan_amount_to_mojos(PlanAmount::new(100), 1_000), 100_000);
        assert_eq!(plan_amount_to_mojos(PlanAmount::new(5), 1_000), 5_000);
    }

    #[test]
    fn vault_output_mojos_from_plan_amounts_is_identity() {
        let amounts = [PlanAmount::new(10_010), PlanAmount::new(100)];
        assert_eq!(
            vault_output_mojos_from_plan_amounts(&amounts).expect("mojos"),
            vec![10_010, 100]
        );
    }

    #[test]
    fn bootstrap_overshoot_change_uses_mojo_multiplier() {
        assert_eq!(
            bootstrap_overshoot_change_mojos(PlanAmount::new(105), PlanAmount::new(100), 1_000),
            5_000
        );
    }

    #[test]
    fn eco181_shape_outputs_target_not_selected_total() {
        let inputs = crate::coin_ops::shape::CombineInputs {
            input_coin_ids: vec!["a".repeat(64), "b".repeat(64)],
            selected_total: 105,
            target_amount: 100,
            exact_match: false,
            cap_applied: true,
            selected_count_before_cap: 2,
            combine_input_cap: 5,
        };
        let outputs = bootstrap_combine_vault_outputs(&inputs, 1_000).expect("outputs");
        assert_eq!(outputs, vec![100_000]);
        let as_mojos = bootstrap_combine_vault_outputs_as_mojos(&inputs).expect("mojos");
        assert_eq!(as_mojos, vec![100]);
    }
}
