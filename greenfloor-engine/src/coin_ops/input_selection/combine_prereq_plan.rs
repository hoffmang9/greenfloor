//! Test-only daemon combine-first coverage of [`crate::coin_ops::shape::plan_combine_inputs_for_target`].
//!
//! Production daemon combine-first funding goes through
//! `resolve_shape_funding`/`ShapeFundingOptions::daemon_unprotected` (see
//! `super::auto_split`); this thin wrapper exists only so unit tests can exercise the
//! shared flat-combine core directly with mojo-denominated [`SpendableCoin`] inputs.

use crate::coin_ops::selection::SpendableCoin;
use crate::coin_ops::shape::{plan_combine_inputs_for_target, CombineInputs, ShapeCoin};

pub(super) fn build_combine_prereq_plan(
    candidate_spendable: &[SpendableCoin],
    target_amount_mojos: i64,
    combine_input_cap: i64,
) -> Option<CombineInputs> {
    let coins: Vec<ShapeCoin> = candidate_spendable
        .iter()
        .map(|coin| ShapeCoin::new(coin.id.clone(), coin.amount))
        .collect();
    plan_combine_inputs_for_target(&coins, target_amount_mojos, combine_input_cap)
}
