//! ECO.181 post-combine cycle expectations for strategy and coin-op planning.

use crate::coin_ops::{plan_coin_ops, BucketSpec, CoinOpKind, CoinOpPlanReason};
use crate::cycle::{evaluate_market, MarketState, StrategyConfig};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_plans_multiple_sizes_from_empty_state() {
        let config = StrategyConfig {
            pair: "wusdbc".to_string(),
            ones_target: 5,
            tens_target: 2,
            hundreds_target: 1,
            target_spread_bps: None,
            min_xch_price_usd: None,
            max_xch_price_usd: None,
            offer_expiry_minutes: Some(120),
            target_counts_by_size: None,
        };
        let state = MarketState {
            ones: 0,
            tens: 0,
            hundreds: 0,
            xch_price_usd: None,
            bucket_counts_by_size: None,
        };
        let actions = evaluate_market(&state, &config);
        assert!(actions.len() >= 3);
        assert!(actions.iter().any(|action| action.size == 100));
    }

    #[test]
    fn coin_ops_plans_ten_bu_buffer_split_after_eco181_combine() {
        let coin_ops = plan_coin_ops(
            &[
                BucketSpec {
                    size_base_units: 1,
                    target_count: 5,
                    split_buffer_count: 1,
                    combine_when_excess_factor: 2.0,
                    current_count: 9,
                },
                BucketSpec {
                    size_base_units: 10,
                    target_count: 2,
                    split_buffer_count: 1,
                    combine_when_excess_factor: 2.0,
                    current_count: 2,
                },
                BucketSpec {
                    size_base_units: 100,
                    target_count: 1,
                    split_buffer_count: 0,
                    combine_when_excess_factor: 2.0,
                    current_count: 1,
                },
            ],
            20,
            100,
            0,
            0,
        );
        assert_eq!(coin_ops.plans.len(), 1);
        assert_eq!(coin_ops.plans[0].op_type, CoinOpKind::Split);
        assert_eq!(coin_ops.plans[0].size_base_units, 10);
        assert_eq!(
            coin_ops.plans[0].reason,
            CoinOpPlanReason::LowWatermarkBufferDeficit
        );
    }
}
