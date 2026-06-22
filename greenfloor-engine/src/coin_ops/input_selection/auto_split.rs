use std::collections::HashSet;

use super::types::{
    CliSplitSelection, SplitAutoSelectPlan, SplitCoinPlan, SplitSkipReason, SubCatChangeSkipData,
};
use crate::coin_ops::policy::coin_op_min_amount_mojos;
use crate::coin_ops::selection::{
    select_largest_spendable_coin, split_would_create_sub_cat_change, SpendableCoin,
};

use super::combine_prereq_plan::build_combine_prereq_plan;

fn skip_no_spendable_coin() -> SplitSkipReason {
    SplitSkipReason::NoSpendableMeetsRequired
}

fn coin_plan(coin: &SpendableCoin) -> SplitCoinPlan {
    SplitCoinPlan {
        coin_id: coin.id.clone(),
        selected_amount_mojos: coin.amount,
    }
}

/// CLI auto split: pick the largest spendable coin without enforcing required amount.
#[must_use]
pub fn plan_cli_auto_split_selection(candidate_spendable: &[SpendableCoin]) -> CliSplitSelection {
    if let Some(selected_coin) =
        select_largest_spendable_coin(candidate_spendable, 0, &HashSet::new())
    {
        return CliSplitSelection::Coin(coin_plan(selected_coin));
    }
    CliSplitSelection::Skip(skip_no_spendable_coin())
}

/// Daemon auto split: enforce required amount, optional combine prereq, and sub-CAT dust checks.
#[must_use]
pub fn plan_daemon_auto_split_selection(
    candidate_spendable: &[SpendableCoin],
    required_amount_mojos: i64,
    canonical_asset_id: &str,
    combine_input_cap: i64,
    allow_combine_prereq: bool,
) -> SplitAutoSelectPlan {
    let min_amount = if required_amount_mojos > 0 {
        required_amount_mojos
    } else {
        0
    };

    if let Some(selected_coin) =
        select_largest_spendable_coin(candidate_spendable, min_amount, &HashSet::new())
    {
        let selected_amount = selected_coin.amount;
        let (would_create_dust, remainder) = split_would_create_sub_cat_change(
            selected_amount,
            required_amount_mojos,
            canonical_asset_id,
        );
        if would_create_dust {
            return SplitAutoSelectPlan::Skip(SplitSkipReason::SubCatChange(
                SubCatChangeSkipData {
                    selected_coin_id: selected_coin.id.clone(),
                    selected_amount_mojos: selected_amount,
                    required_amount_mojos,
                    remainder_mojos: remainder,
                    minimum_allowed_mojos: coin_op_min_amount_mojos(canonical_asset_id),
                },
            ));
        }
        return SplitAutoSelectPlan::Coin(coin_plan(selected_coin));
    }

    if allow_combine_prereq && required_amount_mojos > 0 {
        let aggregate: i64 = candidate_spendable.iter().map(|coin| coin.amount).sum();
        if aggregate >= required_amount_mojos {
            if let Some(prereq) = build_combine_prereq_plan(
                candidate_spendable,
                required_amount_mojos,
                combine_input_cap,
            ) {
                return SplitAutoSelectPlan::CombinePrereq(prereq);
            }
        }
    }

    SplitAutoSelectPlan::Skip(skip_no_spendable_coin())
}
