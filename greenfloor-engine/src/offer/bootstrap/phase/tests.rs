use super::{
    bootstrap_early_phase, bootstrap_executed_phase, resolve_bootstrap_wait_poll,
    BootstrapPhaseStatus, BootstrapWaitContext, BootstrapWaitPoll, BootstrapWaitResolution,
};
use crate::offer::bootstrap::test_fixtures::{
    bootstrap_coin as coin, ladder_row as row, plan_bootstrap,
};
use crate::offer::bootstrap::{BootstrapCoin, BootstrapPlanOutcome, PlannerLadderRow};

fn after_combine_poll<'a>(
    combine_target_amount: i64,
    ladder: &'a [PlannerLadderRow],
    spendable: &'a [BootstrapCoin],
) -> BootstrapWaitPoll<'a> {
    BootstrapWaitPoll::AfterCombine(BootstrapWaitContext {
        combine_target_amount,
        ladder_entries: ladder,
        spendable_coins: spendable,
    })
}

#[test]
fn early_phase_skips_when_needs_split() {
    let ladder = vec![row(10, 2, 0)];
    let spendable = vec![coin("coin-big", 100)];
    let outcome = plan_bootstrap(&ladder, &spendable);
    assert!(bootstrap_early_phase(&outcome, &ladder, &spendable).is_none());
}

#[test]
fn early_phase_reports_already_ready() {
    let ladder = vec![row(10, 2, 0)];
    let spendable = vec![coin("coin-a", 10), coin("coin-b", 10)];
    let phase = bootstrap_early_phase(&BootstrapPlanOutcome::Ready, &ladder, &spendable)
        .expect("ready snapshot");
    assert_eq!(phase.status, BootstrapPhaseStatus::Skipped);
    assert_eq!(phase.reason, "already_ready");
    assert!(!phase.ready);
}

#[test]
fn executed_phase_reports_still_underfunded() {
    let remaining = BootstrapPlanOutcome::CannotFund {
        total_output_amount: 20,
    };
    let phase = bootstrap_executed_phase(&remaining);
    assert_eq!(phase.status, BootstrapPhaseStatus::Executed);
    assert!(!phase.ready);
    assert!(phase
        .reason
        .contains("still_underfunded:total_output_amount=20"));
}

#[test]
fn after_combine_wait_completes_when_combine_fully_shapes_ladder() {
    let ladder = vec![row(100, 1, 0)];
    let spendable = vec![coin("combined", 100)];
    let ready = BootstrapPlanOutcome::Ready;
    assert_eq!(
        resolve_bootstrap_wait_poll(after_combine_poll(100, &ladder, &spendable), &ready, false,),
        BootstrapWaitResolution::Complete(ready)
    );
}

#[test]
fn after_combine_wait_not_complete_on_cannot_fund_even_when_inventory_changed() {
    let ladder = vec![row(100, 1, 0)];
    let change_only = vec![coin("change", 5)];
    let outcome = plan_bootstrap(&ladder, &change_only);
    assert_eq!(
        resolve_bootstrap_wait_poll(
            after_combine_poll(100, &ladder, &change_only),
            &outcome,
            true,
        ),
        BootstrapWaitResolution::Continue
    );

    let cannot_fund = BootstrapPlanOutcome::CannotFund {
        total_output_amount: 100,
    };
    assert_eq!(
        resolve_bootstrap_wait_poll(
            after_combine_poll(100, &ladder, &change_only),
            &cannot_fund,
            true,
        ),
        BootstrapWaitResolution::Continue
    );
}

#[test]
fn after_combine_wait_completes_when_single_coin_split_plan_available() {
    let ladder = vec![row(100, 1, 0)];
    let spendable = vec![coin("combined", 100)];
    let outcome = plan_bootstrap(&ladder, &spendable);
    assert!(matches!(
        resolve_bootstrap_wait_poll(
            after_combine_poll(100, &ladder, &spendable),
            &outcome,
            false,
        ),
        BootstrapWaitResolution::Complete(_)
    ));
}

#[test]
fn after_combine_wait_completes_on_ready_outcome() {
    let ladder = vec![row(100, 1, 0)];
    let spendable = vec![coin("combined", 100)];
    assert_eq!(
        resolve_bootstrap_wait_poll(
            after_combine_poll(100, &ladder, &spendable),
            &BootstrapPlanOutcome::Ready,
            false,
        ),
        BootstrapWaitResolution::Complete(BootstrapPlanOutcome::Ready),
    );
}

#[test]
fn after_combine_wait_continues_while_combine_first_still_pending() {
    use crate::test_support::eco181_bootstrap_inventory::{
        eco181_bootstrap_coins, eco181_bootstrap_ladder,
    };

    let ladder = eco181_bootstrap_ladder();
    let coins = eco181_bootstrap_coins();
    let outcome = plan_bootstrap(&ladder, &coins);
    let BootstrapPlanOutcome::NeedsShape(plan) = &outcome else {
        panic!("expected combine-first plan");
    };
    assert!(plan.requires_combine_first());
    assert_eq!(
        resolve_bootstrap_wait_poll(after_combine_poll(100, &ladder, &coins), &outcome, false,),
        BootstrapWaitResolution::Continue,
    );
}

#[test]
fn after_split_wait_completes_on_ready_or_settled_inventory_update() {
    let ready = BootstrapPlanOutcome::Ready;
    assert_eq!(
        resolve_bootstrap_wait_poll(BootstrapWaitPoll::AfterSplit, &ready, false),
        BootstrapWaitResolution::Complete(ready)
    );

    let ladder = vec![row(100, 2, 0)];
    let spendable = vec![coin("combined", 100)];
    let needs_split = plan_bootstrap(&ladder, &spendable);
    assert_eq!(
        resolve_bootstrap_wait_poll(BootstrapWaitPoll::AfterSplit, &needs_split, false),
        BootstrapWaitResolution::Continue
    );
    assert!(matches!(
        resolve_bootstrap_wait_poll(BootstrapWaitPoll::AfterSplit, &needs_split, true),
        BootstrapWaitResolution::Complete(_)
    ));

    let cannot_fund = BootstrapPlanOutcome::CannotFund {
        total_output_amount: 200,
    };
    assert!(matches!(
        resolve_bootstrap_wait_poll(BootstrapWaitPoll::AfterSplit, &cannot_fund, true),
        BootstrapWaitResolution::Complete(_)
    ));
}

#[test]
fn after_split_wait_ignores_combine_first_inventory_updates() {
    let ladder = vec![row(100, 1, 0)];
    let fragmented = vec![
        coin("sixty", 60),
        coin("ten-a", 10),
        coin("ten-b", 10),
        coin("ten-c", 10),
        coin("ten-d", 10),
    ];
    let combine_first = plan_bootstrap(&ladder, &fragmented);
    let BootstrapPlanOutcome::NeedsShape(plan) = &combine_first else {
        panic!("expected combine-first plan, got {combine_first:?}");
    };
    assert!(plan.requires_combine_first());
    assert_eq!(
        resolve_bootstrap_wait_poll(BootstrapWaitPoll::AfterSplit, &combine_first, true),
        BootstrapWaitResolution::Continue
    );
}
