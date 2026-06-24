//! Canonical bootstrap shape policy: split-source protection and post-combine completion.
//!
//! Offer-post bootstrap (`execute_bootstrap_shape`) shapes inventory for the primary ladder row
//! (the largest configured rung — see [`crate::coin_ops::shape_protection`]). Sub-primary buffer
//! deficits are daemon coin-op scope; see [`crate::coin_ops::defer_low_watermark_split_to_post_bootstrap`].

use super::ladder::ladder_shape_context_for_bootstrap;
use super::planner::{BootstrapCoin, BootstrapPlanOutcome, PlannerLadderRow};
use crate::coin_ops::shape_protection::primary_row_satisfied;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BootstrapDeferScope {
    PreflightPrimaryRow,
    AfterCombineTarget { combine_target_amount: i64 },
}

fn spendable_amounts_base_units(spendable_coins: &[BootstrapCoin]) -> Vec<i64> {
    spendable_coins
        .iter()
        .map(|coin| coin.amount.get())
        .collect()
}

/// True when remaining shape work should defer to daemon coin ops instead of bootstrap.
#[must_use]
pub(crate) fn sub_primary_shape_deferred_to_coin_ops(
    outcome: &BootstrapPlanOutcome,
    primary_size: i64,
    primary_satisfied: bool,
) -> bool {
    if matches!(outcome, BootstrapPlanOutcome::Ready) {
        return true;
    }
    if !primary_satisfied {
        return false;
    }
    match outcome {
        BootstrapPlanOutcome::CannotFund {
            total_output_amount,
        } => crate::coin_ops::shape_protection::defer_sub_primary_shape_to_coin_ops(
            *total_output_amount,
            primary_size,
            true,
        ),
        BootstrapPlanOutcome::NeedsShape(plan) if plan.requires_combine_first() => false,
        BootstrapPlanOutcome::NeedsShape(plan) => {
            crate::coin_ops::shape_protection::defer_sub_primary_shape_to_coin_ops(
                plan.total_output_amount,
                primary_size,
                true,
            ) && plan
                .deficits
                .iter()
                .all(|deficit| deficit.size_base_units < primary_size)
        }
        _ => false,
    }
}

fn bootstrap_shape_deferred_to_coin_ops(
    outcome: &BootstrapPlanOutcome,
    ladder_entries: &[PlannerLadderRow],
    spendable_coins: &[BootstrapCoin],
    scope: BootstrapDeferScope,
) -> bool {
    let shape_ctx = ladder_shape_context_for_bootstrap(
        ladder_entries,
        &spendable_amounts_base_units(spendable_coins),
    );
    match scope {
        BootstrapDeferScope::PreflightPrimaryRow => {
            let Some(primary_size) = shape_ctx.primary_row_size() else {
                return matches!(outcome, BootstrapPlanOutcome::Ready);
            };
            sub_primary_shape_deferred_to_coin_ops(
                outcome,
                primary_size,
                shape_ctx.primary_row_satisfied(),
            )
        }
        BootstrapDeferScope::AfterCombineTarget {
            combine_target_amount,
        } => {
            if !ladder_entries
                .iter()
                .any(|row| row.size_base_units == combine_target_amount)
            {
                return false;
            }
            sub_primary_shape_deferred_to_coin_ops(
                outcome,
                combine_target_amount,
                primary_row_satisfied(
                    combine_target_amount,
                    &shape_ctx.protected_slots,
                    &shape_ctx.exact_ladder_counts,
                ),
            )
        }
    }
}

/// True when bootstrap preflight should skip shaping and let daemon coin ops backfill buffers.
#[must_use]
pub(crate) fn bootstrap_preflight_deferred_to_coin_ops(
    outcome: &BootstrapPlanOutcome,
    ladder_entries: &[PlannerLadderRow],
    spendable_coins: &[BootstrapCoin],
) -> bool {
    bootstrap_shape_deferred_to_coin_ops(
        outcome,
        ladder_entries,
        spendable_coins,
        BootstrapDeferScope::PreflightPrimaryRow,
    )
}

/// True when offer bootstrap should stop after combine mints the target ladder row on-chain.
#[must_use]
pub(crate) fn offer_bootstrap_primary_row_complete(
    combine_target_amount: i64,
    outcome: &BootstrapPlanOutcome,
    ladder_entries: &[PlannerLadderRow],
    spendable_coins: &[BootstrapCoin],
) -> bool {
    bootstrap_shape_deferred_to_coin_ops(
        outcome,
        ladder_entries,
        spendable_coins,
        BootstrapDeferScope::AfterCombineTarget {
            combine_target_amount,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{
        bootstrap_preflight_deferred_to_coin_ops, offer_bootstrap_primary_row_complete,
        sub_primary_shape_deferred_to_coin_ops,
    };
    use crate::offer::bootstrap::{
        bootstrap_replan_after_combine, plan_bootstrap_mixed_outputs, BaseUnits, BootstrapCoin,
        BootstrapCombineContext, BootstrapCombineInputs, BootstrapFundingSource, BootstrapPlan,
        BootstrapPlanOutcome, BootstrapReplanAfterCombine, LadderDeficit, PlannerLadderRow,
    };

    fn row(size: i64, target: i64, buffer: i64) -> PlannerLadderRow {
        PlannerLadderRow {
            size_base_units: size,
            target_count: target,
            split_buffer_count: buffer,
        }
    }

    fn coin(id: &str, amount: i64) -> BootstrapCoin {
        BootstrapCoin {
            id: id.to_string(),
            amount: BaseUnits::new(amount),
        }
    }

    #[test]
    fn sub_primary_deferred_rejects_combine_first_for_second_primary_row() {
        let replanned = BootstrapPlanOutcome::NeedsShape(BootstrapPlan {
            funding: BootstrapFundingSource::CombineFirst(BootstrapCombineInputs {
                input_coin_ids: vec!["a".repeat(64), "b".repeat(64)],
                selected_total: BaseUnits::new(105),
                target_amount: BaseUnits::new(100),
                exact_match: false,
                cap_applied: true,
            }),
            output_amounts_base_units: vec![100],
            total_output_amount: 100,
            change_amount: 5,
            deficits: vec![LadderDeficit {
                size_base_units: 100,
                required_count: 2,
                current_count: 1,
                deficit_count: 1,
            }],
        });
        let spendable = vec![coin("first", 100)];
        assert!(!sub_primary_shape_deferred_to_coin_ops(
            &replanned, 100, true
        ));
        assert!(!offer_bootstrap_primary_row_complete(
            100,
            &replanned,
            &[row(100, 2, 0)],
            &spendable,
        ));
        assert!(matches!(
            bootstrap_replan_after_combine(100, replanned, &[row(100, 2, 0)], &spendable),
            BootstrapReplanAfterCombine::Complete(BootstrapPlanOutcome::NeedsShape(_))
        ));
    }

    #[test]
    fn sub_primary_not_deferred_when_primary_row_still_underfunded() {
        let ladder = vec![row(100, 2, 0)];
        let after_one = vec![
            coin("first", 100),
            coin("fifty", 50),
            coin("forty", 40),
            coin("thirty_a", 30),
            coin("thirty_b", 30),
        ];
        let replanned = plan_bootstrap_mixed_outputs(
            &ladder,
            &after_one,
            5,
            &BootstrapCombineContext::for_tests(),
        );
        assert!(!sub_primary_shape_deferred_to_coin_ops(
            &replanned, 100, false
        ));
        assert!(!bootstrap_preflight_deferred_to_coin_ops(
            &replanned, &ladder, &after_one,
        ));
    }

    #[test]
    fn aggregate_thirty_row_still_needs_split_after_combine() {
        let ladder = vec![row(10, 3, 0)];
        let spendable = vec![coin("combined", 30)];
        let replanned = plan_bootstrap_mixed_outputs(
            &ladder,
            &spendable,
            5,
            &BootstrapCombineContext::for_tests(),
        );
        assert!(!offer_bootstrap_primary_row_complete(
            30, &replanned, &ladder, &spendable,
        ));
        assert!(matches!(replanned, BootstrapPlanOutcome::NeedsShape(_)));
    }

    #[test]
    fn sub_primary_deferred_for_post_combine_split_plan() {
        use crate::test_support::eco181_bootstrap_inventory::{
            eco181_after_combine_coins, eco181_bootstrap_ladder,
        };

        let ladder = eco181_bootstrap_ladder();
        let coins = eco181_after_combine_coins();
        let outcome =
            plan_bootstrap_mixed_outputs(&ladder, &coins, 5, &BootstrapCombineContext::for_tests());
        assert!(sub_primary_shape_deferred_to_coin_ops(&outcome, 100, true,));
        assert!(bootstrap_preflight_deferred_to_coin_ops(
            &outcome, &ladder, &coins,
        ));
    }

    #[test]
    fn sub_primary_not_deferred_for_invalid_ladder_outcome() {
        assert!(!sub_primary_shape_deferred_to_coin_ops(
            &BootstrapPlanOutcome::InvalidLadder,
            100,
            true,
        ));
    }

    #[test]
    fn preflight_ready_without_ladder_rows_defers_to_coin_ops() {
        assert!(bootstrap_preflight_deferred_to_coin_ops(
            &BootstrapPlanOutcome::Ready,
            &[],
            &[],
        ));
    }
}
