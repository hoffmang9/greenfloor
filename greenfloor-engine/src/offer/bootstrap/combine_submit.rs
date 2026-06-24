//! Vault output amounts for bootstrap combine and mixed-split submits.

use super::amounts::base_units_to_mojos;
use super::combine_inputs::BootstrapCombineInputs;
use crate::coin_ops::combine_output_amounts;
use crate::error::SignerResult;

/// Single combine output in mojos for bootstrap combine-first (`target_amount` base units).
///
/// Dust and truncation mismatches are enforced by the vault at submit time.
///
/// # Errors
///
/// Returns an error when output encoding fails.
pub(crate) fn bootstrap_combine_vault_outputs(
    inputs: &BootstrapCombineInputs,
    mojo_multiplier: i64,
) -> SignerResult<Vec<u64>> {
    let output_mojos = base_units_to_mojos(inputs.target_amount, mojo_multiplier);
    combine_output_amounts(output_mojos, 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offer::bootstrap::BootstrapCombineInputs;

    #[test]
    fn eco181_shape_outputs_target_not_selected_total() {
        let inputs = BootstrapCombineInputs {
            input_coin_ids: vec!["a".repeat(64), "b".repeat(64)],
            selected_total: 105,
            target_amount: 100,
            exact_match: false,
            cap_applied: true,
        };
        let outputs = bootstrap_combine_vault_outputs(&inputs, 1_000).expect("outputs");
        assert_eq!(outputs, vec![100_000]);
    }
}
