//! Remaining-shape ownership: who finishes ladder shape across bootstrap and daemon coin ops.
//!
//! Path adapters extract local planner/plan facts; this module answers only **yield vs keep**.
//! Funding selectors and dust batching stay path-local (ADR 0021).

use super::plan::{CoinOpKind, CoinOpPlan, CoinOpPlanReason};
use super::selection::SpendableCoin;
use super::shape_protection::spendable_exact_ladder_unit_amounts;

/// Reason tag for a single low-watermark split plan emitted by coin-op planning.
pub const LOW_WATERMARK_BUFFER_DEFICIT: &str = "low_watermark_buffer_deficit";

/// Whether the current path should hand remaining shape work to the other path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeHandoff {
    /// Yield: the other path owns remaining work.
    Yield,
    /// Keep: the calling path continues.
    Keep,
}

impl ShapeHandoff {
    #[must_use]
    pub fn yields(self) -> bool {
        matches!(self, Self::Yield)
    }
}

/// Bootstrap-side remaining work once primary size/satisfaction are known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapRemainingWork {
    /// Planner reports ready — bootstrap releases.
    Ready,
    /// Primary-row combine-first still pending.
    CombineFirst,
    /// `NeedsShape` / `CannotFund` style remaining amount.
    ShapeOrFund {
        remaining_total: i64,
        primary_size: i64,
        all_deficits_below_primary: bool,
    },
    /// Invalid / non-shape outcomes — not an ownership handoff.
    NonShape,
}

/// Remaining shape work is strictly below the primary ladder row.
#[must_use]
pub fn remaining_shape_below_primary_row(remaining_total: i64, primary_size: i64) -> bool {
    remaining_total > 0 && remaining_total < primary_size
}

/// True when aggregate inventory covers `required` but no single coin does.
///
/// Amounts are unit-agnostic (callers pass either whole ladder units or mojos consistently).
#[must_use]
pub fn aggregate_covers_without_single_coin(required: i64, spendable_amounts: &[i64]) -> bool {
    let required = required.max(0);
    if required == 0 {
        return false;
    }
    let aggregate: i64 = spendable_amounts.iter().copied().sum();
    let max_single = spendable_amounts.iter().copied().max().unwrap_or(0);
    aggregate >= required && max_single < required
}

/// Bootstrap handoff: yield means daemon coin ops own remaining work.
#[must_use]
pub fn bootstrap_handoff(primary_satisfied: bool, work: BootstrapRemainingWork) -> ShapeHandoff {
    match work {
        BootstrapRemainingWork::Ready => ShapeHandoff::Yield,
        BootstrapRemainingWork::CombineFirst | BootstrapRemainingWork::NonShape => {
            ShapeHandoff::Keep
        }
        BootstrapRemainingWork::ShapeOrFund {
            remaining_total,
            primary_size,
            all_deficits_below_primary,
        } => {
            if primary_satisfied
                && remaining_shape_below_primary_row(remaining_total, primary_size)
                && all_deficits_below_primary
            {
                ShapeHandoff::Yield
            } else {
                ShapeHandoff::Keep
            }
        }
    }
}

/// Daemon low-watermark handoff: yield means offer-post bootstrap owns the work.
#[must_use]
pub fn daemon_low_watermark_handoff(
    plan: &CoinOpPlan,
    spendable_amounts_whole_units: &[i64],
) -> ShapeHandoff {
    let is_single_low_watermark_split = plan.op_type == CoinOpKind::Split
        && plan.op_count == 1
        && plan.reason == CoinOpPlanReason::LowWatermarkBufferDeficit;
    if is_single_low_watermark_split
        && aggregate_covers_without_single_coin(plan.size_base_units, spendable_amounts_whole_units)
    {
        ShapeHandoff::Yield
    } else {
        ShapeHandoff::Keep
    }
}

/// Daemon low-watermark handoff from live spendable coins.
///
/// Coverage uses **exact whole ladder units** only. Fractional CAT fragments must not trip
/// `bootstrap_primary_shape_deferred` — after primary bootstrap completes, those deficits are
/// daemon coin-op scope.
#[must_use]
pub fn daemon_low_watermark_handoff_from_spendable(
    plan: &CoinOpPlan,
    spendable: &[SpendableCoin],
    base_unit_mojo_multiplier: i64,
) -> ShapeHandoff {
    daemon_low_watermark_handoff(
        plan,
        &spendable_exact_ladder_unit_amounts(spendable, base_unit_mojo_multiplier),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        aggregate_covers_without_single_coin, bootstrap_handoff, daemon_low_watermark_handoff,
        daemon_low_watermark_handoff_from_spendable, remaining_shape_below_primary_row,
        BootstrapRemainingWork, ShapeHandoff,
    };
    use crate::coin_ops::shape_protection::spendable_exact_ladder_unit_amounts;
    use crate::coin_ops::{CoinOpKind, CoinOpPlan, CoinOpPlanReason, SpendableCoin};

    #[test]
    fn remaining_below_primary_requires_strict_gap() {
        assert!(remaining_shape_below_primary_row(30, 100));
        assert!(!remaining_shape_below_primary_row(100, 100));
        assert!(!remaining_shape_below_primary_row(0, 100));
    }

    #[test]
    fn bootstrap_ready_yields() {
        assert_eq!(
            bootstrap_handoff(true, BootstrapRemainingWork::Ready),
            ShapeHandoff::Yield
        );
    }

    #[test]
    fn bootstrap_combine_first_keeps() {
        assert_eq!(
            bootstrap_handoff(true, BootstrapRemainingWork::CombineFirst),
            ShapeHandoff::Keep
        );
    }

    #[test]
    fn bootstrap_sub_primary_deficits_yield() {
        assert_eq!(
            bootstrap_handoff(
                true,
                BootstrapRemainingWork::ShapeOrFund {
                    remaining_total: 30,
                    primary_size: 100,
                    all_deficits_below_primary: true,
                }
            ),
            ShapeHandoff::Yield
        );
    }

    #[test]
    fn bootstrap_underfunded_primary_keeps() {
        assert_eq!(
            bootstrap_handoff(
                false,
                BootstrapRemainingWork::ShapeOrFund {
                    remaining_total: 30,
                    primary_size: 100,
                    all_deficits_below_primary: true,
                }
            ),
            ShapeHandoff::Keep
        );
    }

    #[test]
    fn spendable_exact_ladder_unit_amounts_keeps_whole_units_only() {
        let spendable = vec![
            SpendableCoin::new("clean".to_string(), 10_000),
            SpendableCoin::new("fractional".to_string(), 10_500),
            SpendableCoin::new("sixty_five".to_string(), 65_000),
        ];
        assert_eq!(
            spendable_exact_ladder_unit_amounts(&spendable, 1_000),
            vec![10, 65]
        );
    }

    #[test]
    fn aggregate_covers_without_single_coin_detects_combine_first_inventory() {
        assert!(aggregate_covers_without_single_coin(100, &[65, 20, 11, 4]));
        assert!(!aggregate_covers_without_single_coin(100, &[150, 10]));
        assert!(!aggregate_covers_without_single_coin(100, &[50, 40]));
    }

    #[test]
    fn daemon_lwm_yields_for_combine_first_inventory() {
        let plan = CoinOpPlan {
            op_type: CoinOpKind::Split,
            size_base_units: 100,
            op_count: 1,
            reason: CoinOpPlanReason::LowWatermarkBufferDeficit,
        };
        assert_eq!(
            daemon_low_watermark_handoff(&plan, &[65, 20, 11, 4]),
            ShapeHandoff::Yield
        );
        assert_eq!(
            daemon_low_watermark_handoff(&plan, &[150, 10]),
            ShapeHandoff::Keep
        );
    }

    #[test]
    fn daemon_from_spendable_ignores_fractional_only_mojo_coverage() {
        let plan = CoinOpPlan {
            op_type: CoinOpKind::Split,
            size_base_units: 10,
            op_count: 1,
            reason: CoinOpPlanReason::LowWatermarkBufferDeficit,
        };
        let spendable = vec![
            SpendableCoin::new("a".to_string(), 6_500),
            SpendableCoin::new("b".to_string(), 3_500),
        ];
        assert_eq!(
            daemon_low_watermark_handoff_from_spendable(&plan, &spendable, 1_000),
            ShapeHandoff::Keep
        );
    }

    #[test]
    fn daemon_from_spendable_matches_exact_whole_unit_amounts() {
        let plan = CoinOpPlan {
            op_type: CoinOpKind::Split,
            size_base_units: 100,
            op_count: 1,
            reason: CoinOpPlanReason::LowWatermarkBufferDeficit,
        };
        let spendable = vec![
            SpendableCoin::new("a".to_string(), 65_000),
            SpendableCoin::new("b".to_string(), 35_000),
        ];
        assert_eq!(
            daemon_low_watermark_handoff_from_spendable(&plan, &spendable, 1_000),
            ShapeHandoff::Yield
        );
    }
}
