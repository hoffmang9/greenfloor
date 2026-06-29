//! Lightweight unit-test builders for bootstrap ladder rows and coins.
//!
//! Use this module for simple `PlannerLadderRow` / `BootstrapCoin` construction and
//! shared planner test helpers in bootstrap unit tests. Scenario inventories and
//! integration fixtures live under `crate::test_support` (for example
//! `eco181_bootstrap_inventory`).

use super::planner::plan_bootstrap_mixed_outputs;
use super::{
    BootstrapCoin, BootstrapCombineContext, BootstrapPlan, BootstrapPlanOutcome, LadderDeficit,
    PlannerLadderRow,
};

pub(super) const DEFAULT_BOOTSTRAP_COMBINE_CAP: i64 = 5;

pub(super) fn ladder_row(size: i64, target: i64, buffer: i64) -> PlannerLadderRow {
    PlannerLadderRow {
        size_base_units: size,
        target_count: target,
        split_buffer_count: buffer,
    }
}

pub(super) fn bootstrap_coin(id: &str, amount: i64) -> BootstrapCoin {
    BootstrapCoin {
        id: id.to_string(),
        amount: super::BaseUnits::new(amount),
    }
}

pub(super) fn ladder_deficit(size: i64, required: i64, current: i64) -> LadderDeficit {
    LadderDeficit::new(size, required, current)
}

pub(super) fn bootstrap_test_context() -> BootstrapCombineContext {
    BootstrapCombineContext::for_tests()
}

pub(super) fn plan_bootstrap(
    ladder: &[PlannerLadderRow],
    spendable: &[BootstrapCoin],
) -> BootstrapPlanOutcome {
    plan_bootstrap_with_cap(ladder, spendable, DEFAULT_BOOTSTRAP_COMBINE_CAP)
}

pub(super) fn plan_bootstrap_with_cap(
    ladder: &[PlannerLadderRow],
    spendable: &[BootstrapCoin],
    combine_cap: i64,
) -> BootstrapPlanOutcome {
    plan_bootstrap_mixed_outputs(ladder, spendable, combine_cap, &bootstrap_test_context())
}

pub(super) fn expect_needs_shape(
    ladder: &[PlannerLadderRow],
    spendable: &[BootstrapCoin],
) -> BootstrapPlan {
    expect_needs_shape_with_cap(ladder, spendable, DEFAULT_BOOTSTRAP_COMBINE_CAP)
}

pub(super) fn expect_needs_shape_with_cap(
    ladder: &[PlannerLadderRow],
    spendable: &[BootstrapCoin],
    combine_cap: i64,
) -> BootstrapPlan {
    match plan_bootstrap_with_cap(ladder, spendable, combine_cap) {
        BootstrapPlanOutcome::NeedsShape(plan) => plan,
        other => panic!("expected needs_shape, got {other:?}"),
    }
}
