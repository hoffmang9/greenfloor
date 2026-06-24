use crate::coin_ops::selection::{
    select_spendable_coins_for_target_amount_with_options, SpendableCoin,
    TargetAmountSelectionOptions,
};
use crate::metrics::metric_non_negative_usize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CombineInputSelection {
    pub input_coin_ids: Vec<String>,
    pub target_amount: i64,
    pub selected_total: i64,
    pub exact_match: bool,
    pub cap_applied: bool,
    pub selected_count_before_cap: usize,
    pub combine_input_cap: i64,
}

/// Select input coins for a combine-first step. Amounts must share one unit system (mojos).
pub(crate) fn select_combine_inputs_for_target(
    candidate_spendable: &[SpendableCoin],
    target_amount: i64,
    combine_input_cap: i64,
) -> Option<CombineInputSelection> {
    if target_amount <= 0 {
        return None;
    }

    let cap = metric_non_negative_usize(combine_input_cap);
    if cap < 2 {
        return None;
    }

    let (unconstrained_ids, unconstrained_total, unconstrained_exact) =
        select_spendable_coins_for_target_amount_with_options(
            candidate_spendable,
            target_amount,
            TargetAmountSelectionOptions::default(),
        );
    let selected_count_before_cap = unconstrained_ids.len();
    if selected_count_before_cap < 2 {
        return None;
    }

    let cap_applied = selected_count_before_cap > cap;
    let (input_coin_ids, selected_total, exact_match) = if cap_applied {
        let (ids, total, exact) = select_spendable_coins_for_target_amount_with_options(
            candidate_spendable,
            target_amount,
            TargetAmountSelectionOptions::combine_cap(cap),
        );
        if ids.is_empty() {
            return None;
        }
        (ids, total, exact)
    } else {
        (unconstrained_ids, unconstrained_total, unconstrained_exact)
    };

    if cap_applied {
        tracing::info!(
            combine_input_cap = cap,
            selected_count_before_cap,
            selected_count = input_coin_ids.len(),
            selected_total,
            required_amount = target_amount,
            exact_match,
            "combine prereq input selection hit combine input cap"
        );
    }

    Some(CombineInputSelection {
        input_coin_ids,
        target_amount,
        selected_total,
        exact_match,
        cap_applied,
        selected_count_before_cap,
        combine_input_cap,
    })
}
