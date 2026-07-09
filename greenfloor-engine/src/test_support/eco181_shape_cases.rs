//! Table-driven ECO.181 bootstrap shape and daemon split-selection expectations.

use crate::coin_ops::shape_protection::SplitSourceProtection;
use crate::coin_ops::{
    plan_daemon_low_watermark_split, DaemonAutoSplitParams, SpendableCoin, SplitAutoSelectPlan,
    SplitSkipReason,
};
use crate::offer::bootstrap::{
    bootstrap_early_phase, bootstrap_phase_snapshot_block_error,
    bootstrap_preflight_deferred_to_coin_ops, bootstrap_replan_after_combine,
    offer_bootstrap_primary_row_complete, plan_bootstrap_mixed_outputs,
    resolve_bootstrap_wait_poll, BootstrapCombineContext, BootstrapPlanOutcome,
    BootstrapReplanAfterCombine, BootstrapWaitContext, BootstrapWaitPoll, BootstrapWaitResolution,
};
use crate::test_support::eco181_bootstrap_inventory::{
    eco181_after_combine_coins, eco181_after_combine_inventory_rows, eco181_bootstrap_coins,
    eco181_bootstrap_ladder, john_deere_after_combine_coins,
    john_deere_after_combine_inventory_rows, john_deere_current_bootstrap_coins,
};
use crate::test_support::ladder::eco181_sell_ladder_entries;

pub const ECO181_MOJO_MULTIPLIER: i64 = 1000;

const TEST_CAT_ASSET: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eco181ShapeExpect {
    CombineFirstPrimary,
    PreflightAlreadyReady,
    PrimaryRowComplete,
    LowWatermarkSplitSkips,
    LowWatermarkSplitFromNinety,
    ReplanAfterCombineReady,
    AfterCombineWaitComplete,
}

pub struct Eco181ShapeCase {
    pub name: &'static str,
    pub expect: Eco181ShapeExpect,
}

#[must_use]
pub fn eco181_shape_cases() -> Vec<Eco181ShapeCase> {
    vec![
        Eco181ShapeCase {
            name: "fragmented_inventory_plans_combine_first",
            expect: Eco181ShapeExpect::CombineFirstPrimary,
        },
        Eco181ShapeCase {
            name: "after_combine_preflight_already_ready",
            expect: Eco181ShapeExpect::PreflightAlreadyReady,
        },
        Eco181ShapeCase {
            name: "after_combine_primary_row_complete",
            expect: Eco181ShapeExpect::PrimaryRowComplete,
        },
        Eco181ShapeCase {
            name: "after_combine_low_watermark_split_skips_primary_row",
            expect: Eco181ShapeExpect::LowWatermarkSplitSkips,
        },
        Eco181ShapeCase {
            name: "john_deere_after_combine_splits_ninety_bu_remnant",
            expect: Eco181ShapeExpect::LowWatermarkSplitFromNinety,
        },
        Eco181ShapeCase {
            name: "replan_after_combine_marks_ready",
            expect: Eco181ShapeExpect::ReplanAfterCombineReady,
        },
        Eco181ShapeCase {
            name: "after_combine_wait_completes_when_buffers_underfunded",
            expect: Eco181ShapeExpect::AfterCombineWaitComplete,
        },
    ]
}

fn combine_context() -> BootstrapCombineContext {
    BootstrapCombineContext::for_tests()
}

fn spendable_mojos(rows: &[(String, i64)]) -> Vec<SpendableCoin> {
    rows.iter()
        .map(|(id, amount)| {
            SpendableCoin::new(id.clone(), amount.saturating_mul(ECO181_MOJO_MULTIPLIER))
        })
        .collect()
}

fn split_params(spendable: &[SpendableCoin]) -> DaemonAutoSplitParams<'_> {
    DaemonAutoSplitParams {
        candidate_spendable: spendable,
        required_amount_mojos: 10 * ECO181_MOJO_MULTIPLIER,
        canonical_asset_id: TEST_CAT_ASSET,
        combine_input_cap: 5,
        allow_combine_prereq: true,
    }
}

fn protection_for_spendable(spendable: &[SpendableCoin]) -> SplitSourceProtection {
    SplitSourceProtection::from_sell_ladder_entries(
        &eco181_sell_ladder_entries(),
        spendable,
        ECO181_MOJO_MULTIPLIER,
    )
}

fn after_combine_wait_poll(
    combine_target_amount: i64,
    ladder: &[crate::offer::bootstrap::PlannerLadderRow],
    spendable: &[crate::offer::bootstrap::BootstrapCoin],
    outcome: &BootstrapPlanOutcome,
) -> BootstrapWaitResolution {
    resolve_bootstrap_wait_poll(
        BootstrapWaitPoll::AfterCombine(BootstrapWaitContext {
            combine_target_amount,
            ladder_entries: ladder,
            spendable_coins: spendable,
        }),
        outcome,
        false,
    )
}

pub fn run_eco181_shape_case(case: &Eco181ShapeCase) {
    let ladder = eco181_bootstrap_ladder();
    match case.expect {
        Eco181ShapeExpect::CombineFirstPrimary => {
            for coins in [
                eco181_bootstrap_coins(),
                john_deere_current_bootstrap_coins(),
            ] {
                let outcome = plan_bootstrap_mixed_outputs(&ladder, &coins, 5, &combine_context());
                let BootstrapPlanOutcome::NeedsShape(plan) = outcome else {
                    panic!(
                        "{}: expected combine-first plan, got {outcome:?}",
                        case.name
                    );
                };
                assert!(plan.requires_combine_first(), "{}", case.name);
                assert_eq!(plan.total_output_amount, 100, "{}", case.name);
            }
        }
        Eco181ShapeExpect::PreflightAlreadyReady => {
            let coins = eco181_after_combine_coins();
            let outcome = plan_bootstrap_mixed_outputs(&ladder, &coins, 5, &combine_context());
            let phase = bootstrap_early_phase(&outcome, &ladder, &coins).expect(case.name);
            assert_eq!(phase.reason, "already_ready", "{}", case.name);
            assert!(
                bootstrap_phase_snapshot_block_error(&phase).is_none(),
                "{}",
                case.name
            );
            assert!(bootstrap_preflight_deferred_to_coin_ops(
                &outcome, &ladder, &coins
            ));
        }
        Eco181ShapeExpect::PrimaryRowComplete => {
            let coins = eco181_after_combine_coins();
            let outcome = plan_bootstrap_mixed_outputs(&ladder, &coins, 5, &combine_context());
            assert!(offer_bootstrap_primary_row_complete(
                100, &outcome, &ladder, &coins,
            ));
        }
        Eco181ShapeExpect::LowWatermarkSplitSkips => {
            let spendable = spendable_mojos(&eco181_after_combine_inventory_rows());
            let protection = protection_for_spendable(&spendable);
            let plan = plan_daemon_low_watermark_split(&split_params(&spendable), &protection);
            assert!(
                matches!(
                    plan,
                    SplitAutoSelectPlan::Skip(SplitSkipReason::NoSpendableMeetsRequired)
                ),
                "{}: {plan:?}",
                case.name
            );
        }
        Eco181ShapeExpect::LowWatermarkSplitFromNinety => {
            let spendable = spendable_mojos(&john_deere_after_combine_inventory_rows());
            let protection = protection_for_spendable(&spendable);
            let plan = plan_daemon_low_watermark_split(&split_params(&spendable), &protection);
            match plan {
                SplitAutoSelectPlan::Coin(coin) => {
                    assert_eq!(coin.coin_id, "ninety", "{}", case.name);
                }
                other => panic!("{}: expected ninety split, got {other:?}", case.name),
            }
            let coins = john_deere_after_combine_coins();
            let outcome = plan_bootstrap_mixed_outputs(&ladder, &coins, 5, &combine_context());
            let phase = bootstrap_early_phase(&outcome, &ladder, &coins).expect(case.name);
            assert_eq!(phase.reason, "already_ready", "{}", case.name);
        }
        Eco181ShapeExpect::ReplanAfterCombineReady => {
            let coins = eco181_after_combine_coins();
            let replanned = plan_bootstrap_mixed_outputs(&ladder, &coins, 5, &combine_context());
            assert!(matches!(
                bootstrap_replan_after_combine(100, replanned, &ladder, &coins),
                BootstrapReplanAfterCombine::Complete(BootstrapPlanOutcome::Ready)
            ));
        }
        Eco181ShapeExpect::AfterCombineWaitComplete => {
            let coins = eco181_after_combine_coins();
            let outcome = plan_bootstrap_mixed_outputs(&ladder, &coins, 5, &combine_context());
            assert!(matches!(
                after_combine_wait_poll(100, &ladder, &coins, &outcome),
                BootstrapWaitResolution::Complete(BootstrapPlanOutcome::Ready)
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{eco181_shape_cases, run_eco181_shape_case};

    #[test]
    fn eco181_shape_table_covers_fragmented_to_post_combine_cycle() {
        for case in eco181_shape_cases() {
            run_eco181_shape_case(&case);
        }
    }
}
