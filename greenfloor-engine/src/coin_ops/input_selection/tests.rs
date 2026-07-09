use std::collections::HashSet;

use super::*;
use crate::coin_ops::selection::SpendableCoin;

fn coins(rows: &[(&str, i64)]) -> Vec<SpendableCoin> {
    rows.iter()
        .map(|(id, amount)| SpendableCoin::new((*id).to_string(), *amount))
        .collect()
}

fn daemon_params<'a>(
    spendable: &'a [SpendableCoin],
    required_amount_mojos: i64,
    canonical_asset_id: &'a str,
    allow_combine_prereq: bool,
) -> DaemonAutoSplitParams<'a> {
    DaemonAutoSplitParams {
        candidate_spendable: spendable,
        required_amount_mojos,
        canonical_asset_id,
        combine_input_cap: 10,
        allow_combine_prereq,
    }
}

#[test]
fn cli_auto_picks_largest_without_required_enforcement() {
    let plan = plan_cli_auto_split_selection(&coins(&[("Coin_small", 100), ("Coin_big", 1500)]));
    match plan {
        CliSplitSelection::Coin(coin) => assert_eq!(coin.coin_id, "Coin_big"),
        CliSplitSelection::Skip(_) => panic!("unexpected skip"),
    }
}

#[test]
fn cli_auto_skips_when_no_spendable_coins() {
    let plan = plan_cli_auto_split_selection(&[]);
    assert!(matches!(plan, CliSplitSelection::Skip(_)));
}

#[test]
fn daemon_auto_requires_single_coin_at_least_required() {
    let spendable = coins(&[("Coin_small", 500), ("Coin_big", 1500)]);
    let plan = plan_daemon_auto_split_selection(&daemon_params(&spendable, 1000, "xch", false));
    match plan {
        SplitAutoSelectPlan::Coin(coin) => assert_eq!(coin.coin_id, "Coin_big"),
        other => panic!("unexpected plan: {other:?}"),
    }
}

#[test]
fn daemon_auto_skips_when_no_coin_meets_required() {
    let spendable = coins(&[("Coin_a", 400), ("Coin_b", 500)]);
    let plan = plan_daemon_auto_split_selection(&daemon_params(&spendable, 1000, "xch", false));
    match plan {
        SplitAutoSelectPlan::Skip(SplitSkipReason::NoSpendableMeetsRequired) => {}
        other => panic!("unexpected plan: {other:?}"),
    }
}

#[test]
fn daemon_auto_returns_combine_prereq_when_aggregate_covers() {
    let spendable = coins(&[("Coin_a", 4000), ("Coin_b", 6000)]);
    let plan = plan_daemon_auto_split_selection(&daemon_params(&spendable, 10_000, "xch", true));
    match plan {
        SplitAutoSelectPlan::CombinePrereq(prereq) => {
            assert_eq!(
                prereq
                    .input_coin_ids
                    .iter()
                    .cloned()
                    .collect::<HashSet<_>>(),
                HashSet::from(["Coin_a".to_string(), "Coin_b".to_string()])
            );
            assert!(prereq.exact_match);
        }
        other => panic!("unexpected plan: {other:?}"),
    }
}

#[test]
fn cli_auto_picks_largest_when_no_single_coin_meets_required() {
    let plan = plan_cli_auto_split_selection(&coins(&[("Coin_a", 4000), ("Coin_b", 6000)]));
    match plan {
        CliSplitSelection::Coin(coin) => assert_eq!(coin.coin_id, "Coin_b"),
        CliSplitSelection::Skip(_) => panic!("unexpected skip"),
    }
}

#[test]
fn daemon_retry_second_attempt_disables_combine_prereq() {
    let spendable = coins(&[("Coin_a", 4000), ("Coin_b", 6000)]);
    let first = plan_daemon_auto_split_selection(&daemon_params(&spendable, 10_000, "xch", true));
    match first {
        SplitAutoSelectPlan::CombinePrereq(_) => {}
        other => panic!("expected combine prereq on first attempt: {other:?}"),
    }
    let second = plan_daemon_auto_split_selection(&daemon_params(&spendable, 10_000, "xch", false));
    match second {
        SplitAutoSelectPlan::Skip(SplitSkipReason::NoSpendableMeetsRequired) => {}
        other => panic!("expected skip when combine prereq disabled: {other:?}"),
    }
}

#[test]
fn combine_exact_amount_skips_non_matching_denominations() {
    let spendable = coins(&[
        ("dust", 500),
        ("coin_a", 1000),
        ("coin_b", 1000),
        ("coin_c", 1000),
    ]);
    let ids = plan_exact_amount_combine_inputs(&spendable, 3, 1000, None, Some(3));
    assert_eq!(ids, vec!["coin_a", "coin_b", "coin_c"]);
}

#[test]
fn combine_exact_amount_normalizes_exclude_ids() {
    let spendable = coins(&[("CoinA", 1000), ("coin_b", 1000), ("coin_c", 1000)]);
    let excluded = HashSet::from(["CoinA".to_string()]);
    let ids = plan_exact_amount_combine_inputs(&spendable, 3, 1000, Some(&excluded), Some(3));
    assert_eq!(ids, vec!["coin_b", "coin_c"]);
}

#[test]
fn combine_largest_by_amount_picks_top_coins_respecting_exclude() {
    let spendable = coins(&[
        ("small", 100),
        ("medium", 500),
        ("big", 1500),
        ("excluded", 2000),
    ]);
    let excluded = HashSet::from(["EXCLUDED".to_string()]);
    let ids = plan_largest_combine_inputs(&spendable, 2, Some(&excluded), None);
    assert_eq!(ids, vec!["big", "medium"]);
}

#[test]
fn daemon_auto_rejects_sub_cat_change_dust() {
    let cat_id = "0000000000000000000000000000000000000000000000000000000000000001";
    let spendable = coins(&[("Coin_cat", 10_500)]);
    let plan = plan_daemon_auto_split_selection(&daemon_params(&spendable, 10_000, cat_id, false));
    match plan {
        SplitAutoSelectPlan::Skip(SplitSkipReason::SubCatChange(data)) => {
            assert_eq!(data.remainder_mojos, 500);
        }
        other => panic!("unexpected plan: {other:?}"),
    }
}

#[test]
fn combine_prereq_plan_returns_none_for_single_coin() {
    let plan = build_combine_prereq_plan(&coins(&[("only", 15_000)]), 10_000, 10);
    assert!(plan.is_none());
}

#[test]
fn combine_prereq_plan_returns_none_when_cap_reduces_below_two_inputs() {
    let spendable = coins(&[("Coin_a", 4000), ("Coin_b", 6000)]);
    assert!(build_combine_prereq_plan(&spendable, 10_000, 1).is_none());
}

#[test]
fn combine_prereq_plan_returns_none_when_cap_truncates_below_required_total() {
    let spendable = coins(&[
        ("Coin_a", 6500),
        ("Coin_b", 2000),
        ("Coin_c", 1100),
        ("Coin_d", 400),
    ]);
    assert!(build_combine_prereq_plan(&spendable, 10_000, 2).is_none());
    assert!(build_combine_prereq_plan(&spendable, 10_000, 3).is_none());
    assert!(build_combine_prereq_plan(&spendable, 10_000, 4).is_some());
}

#[test]
fn combine_prereq_plan_exact_match_when_cap_covers_all_inputs() {
    let spendable = coins(&[("Coin_a", 4000), ("Coin_b", 6000)]);
    let plan = build_combine_prereq_plan(&spendable, 10_000, 10).expect("prereq plan");
    assert_eq!(plan.input_coin_ids.len(), 2);
    assert!(!plan.cap_applied);
    assert!(plan.exact_match);
    assert_eq!(plan.target_amount_mojos, 10_000);
}

#[test]
fn daemon_auto_skips_combine_prereq_when_overshoot_would_be_cat_dust() {
    let cat_id = "0000000000000000000000000000000000000000000000000000000000000001";
    let spendable = coins(&[("Coin_a", 6000), ("Coin_b", 4500)]);
    let plan = plan_daemon_auto_split_selection(&daemon_params(&spendable, 10_000, cat_id, true));
    match plan {
        SplitAutoSelectPlan::Skip(SplitSkipReason::SubCatChange(data)) => {
            assert_eq!(data.remainder_mojos, 500);
        }
        other => panic!("unexpected plan: {other:?}"),
    }
}
