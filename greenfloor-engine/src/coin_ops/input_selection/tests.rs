use std::collections::HashSet;

use super::*;
use crate::coin_ops::selection::SpendableCoin;

fn coins(rows: &[(&str, i64)]) -> Vec<SpendableCoin> {
    rows.iter()
        .map(|(id, amount)| SpendableCoin {
            id: (*id).to_string(),
            amount: *amount,
        })
        .collect()
}

#[test]
fn cli_auto_picks_largest_without_required_enforcement() {
    let plan = plan_cli_auto_split_selection(&coins(&[("Coin_small", 100), ("Coin_big", 1500)]));
    match plan {
        SplitAutoSelectPlan::Coin(coin) => assert_eq!(coin.coin_id, "Coin_big"),
        other => panic!("unexpected plan: {other:?}"),
    }
}

#[test]
fn daemon_auto_requires_single_coin_at_least_required() {
    let plan = plan_daemon_auto_split_selection(
        &coins(&[("Coin_small", 500), ("Coin_big", 1500)]),
        1000,
        "xch",
        10,
        false,
    );
    match plan {
        SplitAutoSelectPlan::Coin(coin) => assert_eq!(coin.coin_id, "Coin_big"),
        other => panic!("unexpected plan: {other:?}"),
    }
}

#[test]
fn daemon_auto_skips_when_no_coin_meets_required() {
    let plan = plan_daemon_auto_split_selection(
        &coins(&[("Coin_a", 400), ("Coin_b", 500)]),
        1000,
        "xch",
        10,
        false,
    );
    match plan {
        SplitAutoSelectPlan::Skip(SplitSkipReason::NoSpendableMeetsRequired) => {}
        other => panic!("unexpected plan: {other:?}"),
    }
}

#[test]
fn daemon_auto_returns_combine_prereq_when_aggregate_covers() {
    let plan = plan_daemon_auto_split_selection(
        &coins(&[("Coin_a", 4000), ("Coin_b", 6000)]),
        10_000,
        "xch",
        10,
        true,
    );
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
fn cli_auto_does_not_return_combine_prereq() {
    let plan = plan_cli_auto_split_selection(&coins(&[("Coin_a", 4000), ("Coin_b", 6000)]));
    match plan {
        SplitAutoSelectPlan::Coin(coin) => assert_eq!(coin.coin_id, "Coin_b"),
        other => panic!("unexpected plan: {other:?}"),
    }
}

#[test]
fn daemon_retry_second_attempt_disables_combine_prereq() {
    let spendable = coins(&[("Coin_a", 4000), ("Coin_b", 6000)]);
    let first = plan_daemon_auto_split_selection(&spendable, 10_000, "xch", 10, true);
    match first {
        SplitAutoSelectPlan::CombinePrereq(_) => {}
        other => panic!("expected combine prereq on first attempt: {other:?}"),
    }
    let second = plan_daemon_auto_split_selection(&spendable, 10_000, "xch", 10, false);
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
    let ids = plan_auto_combine_inputs(
        &spendable,
        3,
        CombineInputSelectionMode::ExactAmount,
        Some(1000),
        None::<&HashSet<String>>,
        Some(3),
    )
    .expect("combine inputs");
    assert_eq!(ids, vec!["coin_a", "coin_b", "coin_c"]);
}

#[test]
fn combine_exact_amount_normalizes_exclude_ids() {
    let spendable = coins(&[("CoinA", 1000), ("coin_b", 1000), ("coin_c", 1000)]);
    let excluded = HashSet::from(["CoinA".to_string()]);
    let ids = plan_auto_combine_inputs(
        &spendable,
        3,
        CombineInputSelectionMode::ExactAmount,
        Some(1000),
        Some(&excluded),
        Some(3),
    )
    .expect("combine inputs");
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
    let ids = plan_auto_combine_inputs(
        &spendable,
        2,
        CombineInputSelectionMode::LargestByAmount,
        None,
        Some(&excluded),
        None,
    )
    .expect("combine inputs");
    assert_eq!(ids, vec!["big", "medium"]);
}

#[test]
fn daemon_auto_rejects_sub_cat_change_dust() {
    let cat_id = "0000000000000000000000000000000000000000000000000000000000000001";
    let plan = plan_daemon_auto_split_selection(
        &coins(&[("Coin_cat", 10_500)]),
        10_000,
        cat_id,
        10,
        false,
    );
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
fn combine_prereq_plan_applies_cap_and_marks_partial_selection() {
    let spendable = coins(&[("Coin_a", 4000), ("Coin_b", 6000)]);
    let plan = build_combine_prereq_plan(&spendable, 10_000, 1).expect("prereq plan");
    assert_eq!(plan.input_coin_ids.len(), 1);
    assert!(plan.cap_applied);
    assert_eq!(plan.selected_count_before_cap, 2);
    assert!(!plan.exact_match);
    assert!(plan.selected_total < 10_000);
}

#[test]
fn combine_prereq_plan_exact_match_when_cap_covers_all_inputs() {
    let spendable = coins(&[("Coin_a", 4000), ("Coin_b", 6000)]);
    let plan = build_combine_prereq_plan(&spendable, 10_000, 10).expect("prereq plan");
    assert_eq!(plan.input_coin_ids.len(), 2);
    assert!(!plan.cap_applied);
    assert!(plan.exact_match);
    assert_eq!(plan.target_amount, 10_000);
}
