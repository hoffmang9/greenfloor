use super::combine_prereq_plan::build_combine_prereq_plan;
use super::types::{
    CliSplitSelection, DaemonAutoSplitParams, SplitAutoSelectPlan, SplitCoinPlan, SplitSkipReason,
    SubCatChangeSkipData,
};
use crate::coin_ops::policy::coin_op_min_amount_mojos;
use crate::coin_ops::selection::{
    select_largest_spendable_coin, split_would_create_sub_cat_change, SpendableCoin,
};
use crate::coin_ops::shape_protection::SplitSourceProtection;

use std::collections::HashSet;

fn skip_no_spendable_coin() -> SplitSkipReason {
    SplitSkipReason::NoSpendableMeetsRequired
}

fn coin_plan(coin: &SpendableCoin) -> SplitCoinPlan {
    SplitCoinPlan {
        coin_id: coin.id.clone(),
        selected_amount_mojos: coin.amount,
    }
}

fn sub_cat_change_skip(
    selected_coin_id: String,
    selected_amount_mojos: i64,
    required_amount_mojos: i64,
    canonical_asset_id: &str,
) -> Option<SplitSkipReason> {
    let (would_create_dust, remainder) = split_would_create_sub_cat_change(
        selected_amount_mojos,
        required_amount_mojos,
        canonical_asset_id,
    );
    if !would_create_dust {
        return None;
    }
    Some(SplitSkipReason::SubCatChange(SubCatChangeSkipData {
        selected_coin_id,
        selected_amount_mojos,
        required_amount_mojos,
        remainder_mojos: remainder,
        minimum_allowed_mojos: coin_op_min_amount_mojos(canonical_asset_id),
    }))
}

fn plan_daemon_auto_split_with_optional_protection(
    params: &DaemonAutoSplitParams<'_>,
    protection: Option<&SplitSourceProtection>,
) -> SplitAutoSelectPlan {
    let min_amount = if params.required_amount_mojos > 0 {
        params.required_amount_mojos
    } else {
        0
    };
    let exclude = HashSet::new();
    let selected_coin = if let Some(protection) = protection {
        let required_base_units =
            params.required_amount_mojos / protection.base_unit_mojo_multiplier.max(1);
        protection.select_spendable_coin(params.candidate_spendable, required_base_units, &exclude)
    } else {
        select_largest_spendable_coin(params.candidate_spendable, min_amount, &exclude)
    };

    if let Some(selected_coin) = selected_coin {
        if let Some(skip) = sub_cat_change_skip(
            selected_coin.id.clone(),
            selected_coin.amount,
            params.required_amount_mojos,
            params.canonical_asset_id,
        ) {
            return SplitAutoSelectPlan::Skip(skip);
        }
        return SplitAutoSelectPlan::Coin(coin_plan(selected_coin));
    }

    if params.allow_combine_prereq && params.required_amount_mojos > 0 {
        let aggregate: i64 = params
            .candidate_spendable
            .iter()
            .map(|coin| coin.amount)
            .sum();
        if aggregate >= params.required_amount_mojos {
            if let Some(prereq) = build_combine_prereq_plan(
                params.candidate_spendable,
                params.required_amount_mojos,
                params.combine_input_cap,
            ) {
                if let Some(skip) = sub_cat_change_skip(
                    prereq.input_coin_ids.first().cloned().unwrap_or_default(),
                    prereq.selected_total_mojos,
                    prereq.target_amount_mojos,
                    params.canonical_asset_id,
                ) {
                    return SplitAutoSelectPlan::Skip(skip);
                }
                return SplitAutoSelectPlan::CombinePrereq(prereq);
            }
        }
    }

    SplitAutoSelectPlan::Skip(skip_no_spendable_coin())
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
pub fn plan_daemon_auto_split_selection(params: &DaemonAutoSplitParams<'_>) -> SplitAutoSelectPlan {
    plan_daemon_auto_split_with_optional_protection(params, None)
}

/// Low-watermark daemon split with ladder-row cannibalization protection.
#[must_use]
pub fn plan_daemon_low_watermark_split(
    params: &DaemonAutoSplitParams<'_>,
    protection: &SplitSourceProtection,
) -> SplitAutoSelectPlan {
    plan_daemon_auto_split_with_optional_protection(params, Some(protection))
}
