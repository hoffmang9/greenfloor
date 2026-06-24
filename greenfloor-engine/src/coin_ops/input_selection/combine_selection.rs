//! Unit-agnostic target-amount coin selection for combine-first steps.
//!
//! Every coin in `coins` and the `target` argument must use the same unit system for
//! the call: on-chain **mojos** (daemon coin ops) or ladder **base units** (bootstrap).

use std::collections::HashSet;

use crate::coin_ops::selection::{
    select_spendable_coins_for_target_amount_with_options, SpendableCoin,
    TargetAmountSelectionOptions,
};
use crate::metrics::metric_non_negative_usize;

/// One coin row for [`select_combine_inputs_for_target`] (unit system chosen by caller).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TargetAmountCoin {
    pub id: String,
    pub amount: i64,
}

/// Selected inputs covering a target amount (amounts share the caller's unit system).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TargetAmountCoinSelection {
    pub input_coin_ids: Vec<String>,
    pub target: i64,
    pub selected_total: i64,
    pub exact_match: bool,
    pub cap_applied: bool,
    pub selected_count_before_cap: usize,
    pub combine_input_cap: i64,
}

pub(crate) fn select_combine_inputs_for_target(
    coins: &[TargetAmountCoin],
    target: i64,
    combine_input_cap: i64,
) -> Option<TargetAmountCoinSelection> {
    select_combine_inputs_for_target_in(coins, target, combine_input_cap, None)
}

pub(crate) fn select_combine_inputs_for_target_in(
    coins: &[TargetAmountCoin],
    target: i64,
    combine_input_cap: i64,
    allowed_coin_ids: Option<&HashSet<String>>,
) -> Option<TargetAmountCoinSelection> {
    if target <= 0 {
        return None;
    }

    let cap = metric_non_negative_usize(combine_input_cap);
    if cap < 2 {
        return None;
    }

    let spendable: Vec<SpendableCoin> = coins
        .iter()
        .filter(|coin| {
            allowed_coin_ids.is_none_or(|allowed| allowed.contains(&coin.id))
                && !coin.id.trim().is_empty()
                && coin.amount > 0
        })
        .map(|coin| SpendableCoin {
            id: coin.id.clone(),
            amount: coin.amount,
        })
        .collect();

    let (unconstrained_ids, unconstrained_total, unconstrained_exact) =
        select_spendable_coins_for_target_amount_with_options(
            &spendable,
            target,
            TargetAmountSelectionOptions::default(),
        );
    let selected_count_before_cap = unconstrained_ids.len();
    if selected_count_before_cap < 2 {
        return None;
    }

    let cap_applied = selected_count_before_cap > cap;
    let (input_coin_ids, selected_total, exact_match) = if cap_applied {
        let (ids, total, exact) = select_spendable_coins_for_target_amount_with_options(
            &spendable,
            target,
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
            required_amount = target,
            exact_match,
            "combine prereq input selection hit combine input cap"
        );
    }

    Some(TargetAmountCoinSelection {
        input_coin_ids,
        target,
        selected_total,
        exact_match,
        cap_applied,
        selected_count_before_cap,
        combine_input_cap,
    })
}
