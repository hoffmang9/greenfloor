use super::plan_bootstrap_mixed_outputs;
use crate::offer::bootstrap::test_fixtures::{
    bootstrap_coin as coin, bootstrap_test_context, expect_needs_shape,
    expect_needs_shape_with_cap, ladder_deficit, ladder_row as row, plan_bootstrap,
    plan_bootstrap_with_cap, DEFAULT_BOOTSTRAP_COMBINE_CAP,
};
use crate::offer::bootstrap::{BaseUnits, BootstrapFundingSource, BootstrapPlanOutcome};

#[test]
fn builds_deficit_outputs() {
    let ladder = vec![row(1, 3, 0), row(10, 2, 1), row(100, 1, 0)];
    let spendable = vec![
        coin("coin-small-1", 1),
        coin("coin-big", 1000),
        coin("coin-hundred", 100),
    ];
    let plan = expect_needs_shape(&ladder, &spendable);
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
        plan_bootstrap(&ladder, &spendable),
        BootstrapPlanOutcome::Ready
    );
}

#[test]
fn selects_smallest_non_cannibalizing_funding_coin() {
    let ladder = vec![row(10, 2, 0)];
    let spendable = vec![coin("coin-big-object", 100)];
    let plan = expect_needs_shape(&ladder, &spendable);
    assert_eq!(plan.source_coin_id(), Some("coin-big-object"));
    assert_eq!(plan.output_amounts_base_units, vec![10, 10]);
}

#[test]
fn skips_satisfied_ladder_row_when_smaller_non_ladder_coin_exists() {
    let ladder = vec![row(10, 2, 1), row(100, 1, 0)];
    let spendable = vec![coin("combined", 100), coin("spare", 50), coin("ten", 10)];
    let plan = expect_needs_shape(&ladder, &spendable);
    assert_eq!(plan.source_coin_id(), Some("spare"));
    assert_eq!(plan.total_output_amount, 20);
}

#[test]
fn skips_coins_without_id() {
    let ladder = vec![row(10, 2, 0)];
    let spendable = vec![coin("", 1000), coin("valid", 100)];
    let plan = expect_needs_shape(&ladder, &spendable);
    assert_eq!(plan.source_coin_id(), Some("valid"));
}

#[test]
fn returns_cannot_fund_when_no_funding_coin() {
    let ladder = vec![row(10, 2, 0)];
    let spendable = vec![coin("small", 5)];
    assert_eq!(
        plan_bootstrap(&ladder, &spendable),
        BootstrapPlanOutcome::CannotFund {
            total_output_amount: 20
        }
    );
}

#[test]
fn preserves_deficit_metadata() {
    let ladder = vec![row(10, 2, 1)];
    let spendable = vec![coin("coin-big", 1000)];
    let plan = expect_needs_shape(&ladder, &spendable);
    assert_eq!(plan.deficits, vec![ladder_deficit(10, 3, 0)]);
    assert_eq!(plan.deficits[0].deficit_count(), 3);
}

#[test]
fn empty_ladder_is_invalid() {
    assert_eq!(
        plan_bootstrap(&[], &[coin("x", 1)]),
        BootstrapPlanOutcome::InvalidLadder
    );
}

#[test]
fn single_output_plan_when_only_one_deficit_coin_needed() {
    let ladder = vec![row(10, 1, 0)];
    let spendable = vec![coin("coin-big", 100)];
    let plan = expect_needs_shape(&ladder, &spendable);
    assert_eq!(plan.output_amounts_base_units, vec![10]);
    assert_eq!(plan.total_output_amount, 10);
}

#[test]
fn returns_invalid_ladder_for_non_positive_size_or_negative_fields() {
    for ladder in [row(0, 1, 0), row(-1, 1, 0), row(10, -1, 0), row(10, 1, -1)] {
        assert_eq!(
            plan_bootstrap(&[ladder], &[coin("x", 100)]),
            BootstrapPlanOutcome::InvalidLadder
        );
    }
}

#[test]
fn returns_invalid_coins_for_negative_amount() {
    assert_eq!(
        plan_bootstrap(&[row(10, 1, 0)], &[coin("bad", -5)]),
        BootstrapPlanOutcome::InvalidCoins
    );
}

#[test]
fn change_amount_matches_source_minus_outputs() {
    let plan = expect_needs_shape(&[row(10, 2, 0)], &[coin("coin-big", 100)]);
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
    let plan = expect_needs_shape(&ladder, &spendable);
    assert!(plan.requires_combine_first());
    assert_eq!(plan.total_output_amount, 100);
    assert_eq!(plan.output_amounts_base_units, vec![100]);
    let input_ids = plan
        .combine_inputs()
        .expect("combine inputs")
        .input_coin_ids
        .as_slice();
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
            plan_bootstrap_with_cap(&ladder, &spendable, cap),
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
    let plan = expect_needs_shape_with_cap(&ladder, &spendable, 4);
    assert!(plan.requires_combine_first());
    assert_eq!(
        plan.combine_inputs().expect("inputs").input_coin_ids.len(),
        4
    );
}

#[test]
fn plans_combine_first_for_fragmented_inventory_with_cap_five() {
    let ladder = vec![row(1, 5, 1), row(10, 2, 1), row(100, 1, 0)];
    let spendable: Vec<_> =
        crate::test_support::fragmented_combine_cap_inventory::fragmented_combine_cap_spendable_coins()
            .into_iter()
            .map(|coin_row| coin(&coin_row.id, coin_row.amount))
            .collect();
    let plan = expect_needs_shape_with_cap(&ladder, &spendable, 5);
    assert!(plan.requires_combine_first());
    assert_eq!(plan.total_output_amount, 100);
    let combine = plan.combine_inputs().expect("combine inputs");
    assert_eq!(combine.target_amount, BaseUnits::new(100));
    assert!(combine.selected_total.get() >= 100);
    assert_eq!(plan.change_amount, combine.selected_total.get() - 100);
    let inputs = combine.input_coin_ids.as_slice();
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
    let plan = expect_needs_shape(&ladder, &eco181_bootstrap_coins());
    assert_eq!(plan.total_output_amount, 100);

    let remaining = plan_bootstrap(&ladder, &eco181_after_combine_coins());
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

// Keep a direct import so plan_bootstrap_mixed_outputs stays covered when fixtures delegate.
#[test]
fn plan_bootstrap_mixed_outputs_accepts_explicit_context() {
    assert_eq!(
        plan_bootstrap_mixed_outputs(
            &[row(10, 1, 0)],
            &[coin("coin-10", 10)],
            DEFAULT_BOOTSTRAP_COMBINE_CAP,
            &bootstrap_test_context(),
        ),
        BootstrapPlanOutcome::Ready
    );
}
