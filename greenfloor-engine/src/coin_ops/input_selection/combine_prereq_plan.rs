use super::types::SplitCombinePrereqPlan;
use crate::coin_ops::selection::{
    select_spendable_coins_for_target_amount_with_options, SpendableCoin,
    TargetAmountSelectionOptions,
};
use crate::metrics::metric_non_negative_usize;

const UNCONSTRAINED_PROBE: TargetAmountSelectionOptions = TargetAmountSelectionOptions {
    max_input_count: None,
    min_input_count: 1,
};

fn combine_cap_options(cap: usize) -> TargetAmountSelectionOptions {
    TargetAmountSelectionOptions {
        max_input_count: Some(cap),
        min_input_count: 2,
    }
}

#[must_use]
pub fn build_combine_prereq_plan(
    candidate_spendable: &[SpendableCoin],
    required_amount_mojos: i64,
    combine_input_cap: i64,
) -> Option<SplitCombinePrereqPlan> {
    let required = required_amount_mojos;
    if required <= 0 {
        return None;
    }

    let cap = metric_non_negative_usize(combine_input_cap);
    if cap < 2 {
        return None;
    }

    let (unconstrained_ids, unconstrained_total, unconstrained_exact) =
        select_spendable_coins_for_target_amount_with_options(
            candidate_spendable,
            required,
            UNCONSTRAINED_PROBE,
        );
    let selected_count_before_cap = unconstrained_ids.len();
    if selected_count_before_cap < 2 {
        return None;
    }

    let cap_applied = selected_count_before_cap > cap;
    let (input_coin_ids, selected_total, exact_match) = if cap_applied {
        let (ids, total, exact) = select_spendable_coins_for_target_amount_with_options(
            candidate_spendable,
            required,
            combine_cap_options(cap),
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
            required_amount = required,
            exact_match,
            "combine prereq input selection hit combine input cap"
        );
    }

    Some(SplitCombinePrereqPlan {
        input_coin_ids,
        target_amount: selected_total.min(required),
        selected_total,
        exact_match,
        cap_applied,
        selected_count_before_cap,
        combine_input_cap,
    })
}
