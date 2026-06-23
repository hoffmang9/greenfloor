//! Post-bootstrap vs daemon coin-op shaping coordination.

use super::{CoinOpKind, CoinOpPlan};

/// Reason tag for a single low-watermark split plan emitted by coin-op planning.
pub const LOW_WATERMARK_BUFFER_DEFICIT: &str = "low_watermark_buffer_deficit";

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
        && plan.reason == LOW_WATERMARK_BUFFER_DEFICIT
        && aggregate_covers_without_single_coin(plan.size_base_units, spendable_amounts_base_units)
}

#[cfg(test)]
mod tests {
    use super::{
        aggregate_covers_without_single_coin, defer_low_watermark_split_to_post_bootstrap,
        LOW_WATERMARK_BUFFER_DEFICIT,
    };
    use crate::coin_ops::{CoinOpKind, CoinOpPlan};

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
            reason: LOW_WATERMARK_BUFFER_DEFICIT.to_string(),
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
}
