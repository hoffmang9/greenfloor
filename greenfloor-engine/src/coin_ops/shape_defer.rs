//! Post-bootstrap vs daemon coin-op shaping coordination.
//!
//! When offer-post bootstrap is still shaping the primary ladder row, daemon low-watermark
//! splits defer to bootstrap via [`defer_low_watermark_split_to_post_bootstrap`]. After
//! bootstrap completes a primary row, see `offer::bootstrap::shape_policy`.

use super::unit_convert::{exact_whole_units_from_mojos, mojos_from_whole_units};
use super::{CoinOpKind, CoinOpPlan, CoinOpPlanReason, SpendableCoin};

/// Reason tag for a single low-watermark split plan emitted by coin-op planning.
pub const LOW_WATERMARK_BUFFER_DEFICIT: &str = "low_watermark_buffer_deficit";

/// Exact whole ladder-unit amounts from spendable coins (for bucket / protection counts).
///
/// Fractional CAT coins (e.g. `10_500` mojos = `10.5` units) are valid inventory but are
/// omitted here — they are not an exact ladder clip.
#[must_use]
pub fn spendable_exact_ladder_unit_amounts(
    spendable: &[SpendableCoin],
    base_unit_mojo_multiplier: i64,
) -> Vec<i64> {
    let multiplier = base_unit_mojo_multiplier.max(1);
    spendable
        .iter()
        .filter_map(|coin| exact_whole_units_from_mojos(coin.amount, multiplier))
        .collect()
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

/// Skip daemon execution when offer-post bootstrap will combine+split instead.
///
/// `spendable_amounts` must use the same unit as `plan.size_base_units` (whole ladder units).
#[must_use]
pub fn defer_low_watermark_split_to_post_bootstrap(
    plan: &CoinOpPlan,
    spendable_amounts_whole_units: &[i64],
) -> bool {
    plan.op_type == CoinOpKind::Split
        && plan.op_count == 1
        && plan.reason == CoinOpPlanReason::LowWatermarkBufferDeficit
        && aggregate_covers_without_single_coin(plan.size_base_units, spendable_amounts_whole_units)
}

/// Skip daemon execution using live spendable coins (sync; safe before split orchestration).
///
/// Coverage is evaluated in **mojos** so fractional CAT coins (e.g. `10.5`) count fully.
#[must_use]
pub fn defer_low_watermark_split_from_spendable(
    plan: &CoinOpPlan,
    spendable: &[SpendableCoin],
    base_unit_mojo_multiplier: i64,
) -> bool {
    if plan.op_type != CoinOpKind::Split
        || plan.op_count != 1
        || plan.reason != CoinOpPlanReason::LowWatermarkBufferDeficit
    {
        return false;
    }
    let required_mojos = mojos_from_whole_units(plan.size_base_units, base_unit_mojo_multiplier);
    let spendable_mojos: Vec<i64> = spendable
        .iter()
        .map(|coin| coin.amount)
        .filter(|amount| *amount > 0)
        .collect();
    aggregate_covers_without_single_coin(required_mojos, &spendable_mojos)
}

#[cfg(test)]
mod tests {
    use super::{
        aggregate_covers_without_single_coin, defer_low_watermark_split_from_spendable,
        defer_low_watermark_split_to_post_bootstrap, spendable_exact_ladder_unit_amounts,
    };
    use crate::coin_ops::{CoinOpKind, CoinOpPlan, CoinOpPlanReason, SpendableCoin};

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
    fn defer_low_watermark_split_only_for_single_output_combine_first() {
        let plan = CoinOpPlan {
            op_type: CoinOpKind::Split,
            size_base_units: 100,
            op_count: 1,
            reason: CoinOpPlanReason::LowWatermarkBufferDeficit,
        };
        assert!(defer_low_watermark_split_to_post_bootstrap(
            &plan,
            &[65, 20, 11, 4]
        ));
        assert!(!defer_low_watermark_split_to_post_bootstrap(
            &plan,
            &[150, 10]
        ));
    }

    #[test]
    fn defer_from_spendable_counts_fractional_cat_mojos() {
        let plan = CoinOpPlan {
            op_type: CoinOpKind::Split,
            size_base_units: 100,
            op_count: 1,
            reason: CoinOpPlanReason::LowWatermarkBufferDeficit,
        };
        // 65.5 + 34.5 = 100 CAT units in mojos; neither coin alone covers.
        let spendable = vec![
            SpendableCoin::new("a".to_string(), 65_500),
            SpendableCoin::new("b".to_string(), 34_500),
        ];
        assert!(defer_low_watermark_split_from_spendable(
            &plan, &spendable, 1_000
        ));
    }

    #[test]
    fn defer_from_spendable_matches_whole_unit_mojos() {
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
        assert!(defer_low_watermark_split_from_spendable(
            &plan, &spendable, 1_000
        ));
    }
}
