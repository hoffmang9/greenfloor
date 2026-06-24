//! Post-bootstrap vs daemon coin-op shaping coordination.

use super::{CoinOpKind, CoinOpPlan, CoinOpPlanReason, SpendableCoin};

/// Reason tag for a single low-watermark split plan emitted by coin-op planning.
pub const LOW_WATERMARK_BUFFER_DEFICIT: &str = "low_watermark_buffer_deficit";

/// Convert spendable coin mojos to positive base-unit amounts.
#[must_use]
pub fn spendable_amounts_in_base_units(
    spendable: &[SpendableCoin],
    base_unit_mojo_multiplier: i64,
) -> Vec<i64> {
    let multiplier = base_unit_mojo_multiplier.max(1);
    spendable
        .iter()
        .map(|coin| coin.amount / multiplier)
        .filter(|amount| *amount > 0)
        .collect()
}

/// True when aggregate inventory covers `required_base_units` but no single coin does.
#[must_use]
pub fn aggregate_covers_without_single_coin(
    required_base_units: i64,
    spendable_amounts_base_units: &[i64],
) -> bool {
    let required = required_base_units.max(0);
    if required == 0 {
        return false;
    }
    let aggregate: i64 = spendable_amounts_base_units.iter().copied().sum();
    let max_single = spendable_amounts_base_units
        .iter()
        .copied()
        .max()
        .unwrap_or(0);
    aggregate >= required && max_single < required
}

/// Skip daemon execution when offer-post bootstrap will combine+split instead.
#[must_use]
pub fn defer_low_watermark_split_to_post_bootstrap(
    plan: &CoinOpPlan,
    spendable_amounts_base_units: &[i64],
) -> bool {
    plan.op_type == CoinOpKind::Split
        && plan.op_count == 1
        && plan.reason == CoinOpPlanReason::LowWatermarkBufferDeficit
        && aggregate_covers_without_single_coin(plan.size_base_units, spendable_amounts_base_units)
}

/// Skip daemon execution using live spendable coins (sync; safe before split orchestration).
#[must_use]
pub fn defer_low_watermark_split_from_spendable(
    plan: &CoinOpPlan,
    spendable: &[SpendableCoin],
    base_unit_mojo_multiplier: i64,
) -> bool {
    defer_low_watermark_split_to_post_bootstrap(
        plan,
        &spendable_amounts_in_base_units(spendable, base_unit_mojo_multiplier),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        aggregate_covers_without_single_coin, defer_low_watermark_split_from_spendable,
        defer_low_watermark_split_to_post_bootstrap, spendable_amounts_in_base_units,
    };
    use crate::coin_ops::{CoinOpKind, CoinOpPlan, CoinOpPlanReason, SpendableCoin};

    #[test]
    fn spendable_amounts_in_base_units_divides_by_multiplier() {
        let spendable = vec![SpendableCoin {
            id: "a".to_string(),
            amount: 65_000,
        }];
        assert_eq!(spendable_amounts_in_base_units(&spendable, 1_000), vec![65]);
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
    fn defer_from_spendable_matches_base_unit_amounts() {
        let plan = CoinOpPlan {
            op_type: CoinOpKind::Split,
            size_base_units: 100,
            op_count: 1,
            reason: CoinOpPlanReason::LowWatermarkBufferDeficit,
        };
        let spendable = vec![
            SpendableCoin {
                id: "a".to_string(),
                amount: 65_000,
            },
            SpendableCoin {
                id: "b".to_string(),
                amount: 35_000,
            },
        ];
        assert!(defer_low_watermark_split_from_spendable(
            &plan, &spendable, 1_000
        ));
    }
}
