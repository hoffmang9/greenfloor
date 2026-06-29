use super::plan_bootstrap_mixed_outputs;
use crate::offer::bootstrap::test_fixtures::{bootstrap_coin as coin, ladder_row as row};
use crate::offer::bootstrap::{
    BaseUnits, BootstrapCombineContext, BootstrapFundingSource, BootstrapPlanOutcome, LadderDeficit,
};

const TEST_COMBINE_CAP: i64 = 5;

#[test]
fn builds_deficit_outputs() {
    let ladder = vec![row(1, 3, 0), row(10, 2, 1), row(100, 1, 0)];
    let spendable = vec![
        coin("coin-small-1", 1),
        coin("coin-big", 1000),
        coin("coin-hundred", 100),
    ];
    let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
        &ladder,
        &spendable,
        TEST_COMBINE_CAP,
        &BootstrapCombineContext::for_tests(),
    ) else {
        panic!("expected needs_shape")
    };
    assert!(matches!(
        plan.funding,
        BootstrapFundingSource::SingleCoin { .. }
    ));
    assert_eq!(plan.source_coin_id(), Some("coin-big"));
    let mut outputs = plan.output_amounts_base_units;
    outputs.sort_unstable();
    assert_eq!(outputs, vec![1, 1, 10, 10, 10]);
    assert_eq!(plan.total_output_amount, 32);
}

#[test]
fn returns_ready_when_inventory_satisfied() {
    let ladder = vec![row(1, 1, 0), row(10, 1, 0)];
    let spendable = vec![
        coin("coin-1", 1),
        coin("coin-10", 10),
        coin("coin-extra", 500),
    ];
    assert_eq!(
        plan_bootstrap_mixed_outputs(
            &ladder,
            &spendable,
            TEST_COMBINE_CAP,
            &BootstrapCombineContext::for_tests()
        ),
        BootstrapPlanOutcome::Ready
    );
}

#[test]
fn selects_smallest_non_cannibalizing_funding_coin() {
    let ladder = vec![row(10, 2, 0)];
    let spendable = vec![coin("coin-big-object", 100)];
    let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
        &ladder,
        &spendable,
        TEST_COMBINE_CAP,
        &BootstrapCombineContext::for_tests(),
    ) else {
        panic!("expected needs_shape")
    };
    assert_eq!(plan.source_coin_id(), Some("coin-big-object"));
    assert_eq!(plan.output_amounts_base_units, vec![10, 10]);
}

#[test]
fn skips_satisfied_ladder_row_when_smaller_non_ladder_coin_exists() {
    let ladder = vec![row(10, 2, 1), row(100, 1, 0)];
    let spendable = vec![coin("combined", 100), coin("spare", 50), coin("ten", 10)];
    let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
        &ladder,
        &spendable,
        TEST_COMBINE_CAP,
        &BootstrapCombineContext::for_tests(),
    ) else {
        panic!("expected needs_shape")
    };
    assert_eq!(plan.source_coin_id(), Some("spare"));
    assert_eq!(plan.total_output_amount, 20);
}

#[test]
fn skips_coins_without_id() {
    let ladder = vec![row(10, 2, 0)];
    let spendable = vec![coin("", 1000), coin("valid", 100)];
    let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
        &ladder,
        &spendable,
        TEST_COMBINE_CAP,
        &BootstrapCombineContext::for_tests(),
    ) else {
        panic!("expected needs_shape")
    };
    assert_eq!(plan.source_coin_id(), Some("valid"));
}

#[test]
fn returns_cannot_fund_when_no_funding_coin() {
    let ladder = vec![row(10, 2, 0)];
    let spendable = vec![coin("small", 5)];
    assert_eq!(
        plan_bootstrap_mixed_outputs(
            &ladder,
            &spendable,
            TEST_COMBINE_CAP,
            &BootstrapCombineContext::for_tests()
        ),
        BootstrapPlanOutcome::CannotFund {
            total_output_amount: 20
        }
    );
}

#[test]
fn preserves_deficit_metadata() {
    let ladder = vec![row(10, 2, 1)];
    let spendable = vec![coin("coin-big", 1000)];
    let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
        &ladder,
        &spendable,
        TEST_COMBINE_CAP,
        &BootstrapCombineContext::for_tests(),
    ) else {
        panic!("expected needs_shape")
    };
    assert_eq!(
        plan.deficits,
        vec![LadderDeficit {
            size_base_units: 10,
            required_count: 3,
            current_count: 0,
            deficit_count: 3,
        }]
    );
}

#[test]
fn empty_ladder_is_invalid() {
    assert_eq!(
        plan_bootstrap_mixed_outputs(
            &[],
            &[coin("x", 1)],
            TEST_COMBINE_CAP,
            &BootstrapCombineContext::for_tests()
        ),
        BootstrapPlanOutcome::InvalidLadder
    );
}

#[test]
fn single_output_plan_when_only_one_deficit_coin_needed() {
    let ladder = vec![row(10, 1, 0)];
    let spendable = vec![coin("coin-big", 100)];
    let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
        &ladder,
        &spendable,
        TEST_COMBINE_CAP,
        &BootstrapCombineContext::for_tests(),
    ) else {
        panic!("expected needs_shape")
    };
    assert_eq!(plan.output_amounts_base_units, vec![10]);
    assert_eq!(plan.total_output_amount, 10);
}

#[test]
fn returns_invalid_ladder_for_non_positive_size_or_negative_fields() {
    for ladder in [row(0, 1, 0), row(-1, 1, 0), row(10, -1, 0), row(10, 1, -1)] {
        assert_eq!(
            plan_bootstrap_mixed_outputs(
                &[ladder],
                &[coin("x", 100)],
                TEST_COMBINE_CAP,
                &BootstrapCombineContext::for_tests()
            ),
            BootstrapPlanOutcome::InvalidLadder
        );
    }
}

#[test]
fn returns_invalid_coins_for_negative_amount() {
    assert_eq!(
        plan_bootstrap_mixed_outputs(
            &[row(10, 1, 0)],
            &[coin("bad", -5)],
            TEST_COMBINE_CAP,
            &BootstrapCombineContext::for_tests()
        ),
        BootstrapPlanOutcome::InvalidCoins
    );
}

#[test]
fn change_amount_matches_source_minus_outputs() {
    let ladder = vec![row(10, 2, 0)];
    let spendable = vec![coin("coin-big", 100)];
    let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
        &ladder,
        &spendable,
        TEST_COMBINE_CAP,
        &BootstrapCombineContext::for_tests(),
    ) else {
        panic!("expected needs_shape")
    };
    assert_eq!(
        plan.change_amount,
        plan.source_amount() - plan.total_output_amount
    );
}

#[test]
fn plans_combine_first_when_aggregate_covers_deficit_without_single_coin() {
    let ladder = vec![row(100, 1, 0)];
    let spendable = vec![
        coin("sixty-five", 65),
        coin("twenty", 20),
        coin("eleven", 11),
        coin("four", 4),
    ];
    let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
        &ladder,
        &spendable,
        TEST_COMBINE_CAP,
        &BootstrapCombineContext::for_tests(),
    ) else {
        panic!("expected needs_shape combine-first")
    };
    assert!(plan.requires_combine_first());
    assert_eq!(plan.total_output_amount, 100);
    assert_eq!(plan.output_amounts_base_units, vec![100]);
    let input_ids = plan.combine_input_coin_ids().expect("combine input ids");
    assert!(input_ids.len() >= 2);
    assert!(plan.source_amount() >= 100);
}

#[test]
fn capped_combine_returns_cannot_fund_when_truncated_inputs_are_insufficient() {
    let ladder = vec![row(100, 1, 0)];
    let spendable = vec![
        coin("sixty-five", 65),
        coin("twenty", 20),
        coin("eleven", 11),
        coin("four", 4),
    ];
    for cap in [2, 3] {
        assert_eq!(
            plan_bootstrap_mixed_outputs(
                &ladder,
                &spendable,
                cap,
                &BootstrapCombineContext::for_tests()
            ),
            BootstrapPlanOutcome::CannotFund {
                total_output_amount: 100
            }
        );
    }
}

#[test]
fn capped_combine_succeeds_when_cap_includes_enough_inputs() {
    let ladder = vec![row(100, 1, 0)];
    let spendable = vec![
        coin("sixty-five", 65),
        coin("twenty", 20),
        coin("eleven", 11),
        coin("four", 4),
    ];
    let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
        &ladder,
        &spendable,
        4,
        &BootstrapCombineContext::for_tests(),
    ) else {
        panic!("expected needs_shape with cap=4")
    };
    assert!(plan.requires_combine_first());
    assert_eq!(plan.combine_input_coin_ids().expect("inputs").len(), 4);
}

#[test]
fn plans_combine_first_for_fragmented_inventory_with_cap_five() {
    let ladder = vec![row(1, 5, 1), row(10, 2, 1), row(100, 1, 0)];
    let spendable: Vec<_> =
        crate::test_support::fragmented_combine_cap_inventory::fragmented_combine_cap_spendable_coins()
            .into_iter()
            .map(|coin_row| coin(&coin_row.id, coin_row.amount))
            .collect();
    let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
        &ladder,
        &spendable,
        5,
        &BootstrapCombineContext::for_tests(),
    ) else {
        panic!("expected combine-first plan for fragmented inventory")
    };
    assert!(plan.requires_combine_first());
    assert_eq!(plan.total_output_amount, 100);
    let combine = plan.combine_inputs().expect("combine inputs");
    assert_eq!(combine.target_amount, BaseUnits::new(100));
    assert!(combine.selected_total.get() >= 100);
    assert_eq!(plan.change_amount, combine.selected_total.get() - 100);
    let inputs = plan.combine_input_coin_ids().expect("combine input ids");
    assert!(inputs.len() >= 2);
    assert!(inputs.len() <= 5);
    assert!(plan.source_amount() >= 100);
}

#[test]
fn eco181_inventory_replan_after_combine_preserves_hundred_row() {
    use crate::test_support::eco181_bootstrap_inventory::{
        eco181_after_combine_coins, eco181_bootstrap_coins, eco181_bootstrap_ladder,
    };

    let ladder = eco181_bootstrap_ladder();
    let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
        &ladder,
        &eco181_bootstrap_coins(),
        TEST_COMBINE_CAP,
        &BootstrapCombineContext::for_tests(),
    ) else {
        panic!("expected combine-first plan")
    };
    assert_eq!(plan.total_output_amount, 100);

    let remaining = plan_bootstrap_mixed_outputs(
        &ladder,
        &eco181_after_combine_coins(),
        TEST_COMBINE_CAP,
        &BootstrapCombineContext::for_tests(),
    );
    match remaining {
        BootstrapPlanOutcome::Ready => {}
        BootstrapPlanOutcome::CannotFund {
            total_output_amount,
        } => {
            assert!(
                total_output_amount < 100,
                "100 BU row must stay satisfied after combine: {remaining:?}"
            );
        }
        BootstrapPlanOutcome::NeedsShape(ref split) => {
            assert_ne!(
                split.source_amount(),
                100,
                "must not split the satisfied 100 BU row for smaller deficits: {remaining:?}"
            );
            assert!(
                !split
                    .deficits
                    .iter()
                    .any(|deficit| deficit.size_base_units == 100),
                "100 BU row must stay satisfied after combine: {remaining:?}"
            );
        }
        other => panic!("unexpected post-combine outcome: {other:?}"),
    }
}
