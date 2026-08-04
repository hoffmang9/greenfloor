use super::types::{
    CliSplitSelection, DaemonAutoSplitParams, SplitAutoSelectPlan, SplitCoinPlan, SplitSkipReason,
    SubCatChangeSkipData,
};
use crate::coin_ops::policy::coin_op_min_amount_mojos;
use crate::coin_ops::selection::{
    select_largest_spendable_coin, split_would_create_sub_cat_change, SpendableCoin,
};
use crate::coin_ops::shape::{
    resolve_shape_funding, ShapeCoin, ShapeFunding, ShapeFundingOptions, ShapeFundingResolution,
};
use crate::coin_ops::shape_protection::SplitSourceProtection;

use std::collections::HashSet;

fn skip_no_spendable_coin() -> SplitSkipReason {
    SplitSkipReason::NoSpendableMeetsRequired
}

fn coin_plan(coin_id: &str, amount_mojos: i64) -> SplitCoinPlan {
    SplitCoinPlan {
        coin_id: coin_id.to_string(),
        selected_amount_mojos: amount_mojos,
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

/// Shared daemon auto-split planner via `coin_ops::shape::resolve_shape_funding`. Without
/// protection, picks the **largest** eligible coin; with protection, picks the **smallest
/// non-cannibalizing** coin for ladder-row safety. Combine-first fallback is disabled via
/// `ShapeFundingOptions::allow_combine` when `allow_combine_prereq` is `false`.
fn plan_daemon_auto_split_with_optional_protection(
    params: &DaemonAutoSplitParams<'_>,
    protection: Option<&SplitSourceProtection>,
) -> SplitAutoSelectPlan {
    let coins: Vec<ShapeCoin> = params
        .candidate_spendable
        .iter()
        .map(|coin| ShapeCoin::new(coin.id.clone(), coin.amount))
        .collect();
    let options = match protection {
        Some(protection) => ShapeFundingOptions::daemon_protected(
            params.combine_input_cap,
            params.canonical_asset_id,
            protection.base_unit_mojo_multiplier,
            params.allow_combine_prereq,
        ),
        None => ShapeFundingOptions::daemon_unprotected(
            params.combine_input_cap,
            params.canonical_asset_id,
            params.allow_combine_prereq,
        ),
    };
    let ladder_shape = protection.map(|protection| &protection.shape);

    match resolve_shape_funding(&coins, params.required_amount_mojos, ladder_shape, &options) {
        ShapeFundingResolution::Funded(ShapeFunding::SingleCoin { coin_id, amount }) => {
            if let Some(skip) = sub_cat_change_skip(
                coin_id.clone(),
                amount,
                params.required_amount_mojos,
                params.canonical_asset_id,
            ) {
                return SplitAutoSelectPlan::Skip(skip);
            }
            SplitAutoSelectPlan::Coin(coin_plan(&coin_id, amount))
        }
        ShapeFundingResolution::Funded(ShapeFunding::CombineFirst(prereq)) => {
            if let Some(skip) = sub_cat_change_skip(
                prereq.input_coin_ids.first().cloned().unwrap_or_default(),
                prereq.selected_total,
                prereq.target_amount,
                params.canonical_asset_id,
            ) {
                return SplitAutoSelectPlan::Skip(skip);
            }
            SplitAutoSelectPlan::CombinePrereq(prereq)
        }
        ShapeFundingResolution::CannotFund { .. } => {
            SplitAutoSelectPlan::Skip(skip_no_spendable_coin())
        }
    }
}

/// CLI auto split: pick the largest spendable coin without enforcing required amount.
#[must_use]
pub fn plan_cli_auto_split_selection(candidate_spendable: &[SpendableCoin]) -> CliSplitSelection {
    if let Some(selected_coin) =
        select_largest_spendable_coin(candidate_spendable, 0, &HashSet::new())
    {
        return CliSplitSelection::Coin(coin_plan(&selected_coin.id, selected_coin.amount));
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
