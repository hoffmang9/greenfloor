use crate::offer::pricing::{combine_threshold_count, i64_to_f64};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoinOpKind {
    Split,
    Combine,
}

impl CoinOpKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Split => "split",
            Self::Combine => "combine",
        }
    }

    pub fn fee_mojos(self, split_fee_mojos: i64, combine_fee_mojos: i64) -> i64 {
        match self {
            Self::Split => split_fee_mojos,
            Self::Combine => combine_fee_mojos,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LadderTargetRow {
    pub size_base_units: i64,
    pub target_count: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BucketSpec {
    pub size_base_units: i64,
    pub target_count: i64,
    pub split_buffer_count: i64,
    pub combine_when_excess_factor: f64,
    pub current_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoinOpPlan {
    pub op_type: CoinOpKind,
    pub size_base_units: i64,
    pub op_count: i64,
    pub reason: String,
}

pub fn plan_coin_ops(
    buckets: &[BucketSpec],
    max_operations_per_run: i64,
    max_fee_budget_mojos: i64,
    split_fee_mojos: i64,
    combine_fee_mojos: i64,
) -> Vec<CoinOpPlan> {
    let mut plans = Vec::new();
    let mut remaining_ops = max_operations_per_run;
    let mut remaining_fee = if max_fee_budget_mojos > 0 {
        max_fee_budget_mojos
    } else {
        i64::MAX / 2
    };

    let mut deficits: Vec<(f64, &BucketSpec, i64)> = Vec::new();
    for bucket in buckets {
        let threshold = bucket.target_count + bucket.split_buffer_count;
        let deficit = threshold - bucket.current_count;
        if deficit > 0 && bucket.target_count > 0 {
            deficits.push((
                i64_to_f64(deficit) / i64_to_f64(bucket.target_count),
                bucket,
                deficit,
            ));
        }
    }
    deficits.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.1.size_base_units.cmp(&right.1.size_base_units))
    });

    let had_deficits = !deficits.is_empty();
    for (_ratio, bucket, deficit) in deficits {
        if remaining_ops <= 0 {
            break;
        }
        if split_fee_mojos > remaining_fee {
            break;
        }
        let op_count = deficit.min(remaining_ops);
        if op_count <= 0 {
            continue;
        }
        plans.push(CoinOpPlan {
            op_type: CoinOpKind::Split,
            size_base_units: bucket.size_base_units,
            op_count,
            reason: "low_watermark_buffer_deficit".to_string(),
        });
        remaining_ops -= op_count;
        remaining_fee -= split_fee_mojos;
    }

    if had_deficits {
        return plans;
    }

    let mut excess_candidates: Vec<(&BucketSpec, i64)> = Vec::new();
    for bucket in buckets {
        let Ok(threshold) =
            combine_threshold_count(bucket.target_count, bucket.combine_when_excess_factor)
        else {
            continue;
        };
        let excess = bucket.current_count - threshold;
        if excess > 0 {
            excess_candidates.push((bucket, excess));
        }
    }
    excess_candidates.sort_by_key(|(bucket, _)| bucket.size_base_units);

    for (bucket, excess) in excess_candidates {
        if remaining_ops <= 0 {
            break;
        }
        if combine_fee_mojos > remaining_fee {
            break;
        }
        let op_count = excess.min(remaining_ops);
        if op_count <= 0 {
            continue;
        }
        plans.push(CoinOpPlan {
            op_type: CoinOpKind::Combine,
            size_base_units: bucket.size_base_units,
            op_count,
            reason: "excess_only_policy".to_string(),
        });
        remaining_ops -= op_count;
        remaining_fee -= combine_fee_mojos;
    }

    plans
}

#[cfg(test)]
mod tests {
    use super::{plan_coin_ops, BucketSpec, CoinOpKind};

    fn bucket(size_base_units: i64, target_count: i64, current_count: i64) -> BucketSpec {
        BucketSpec {
            size_base_units,
            target_count,
            split_buffer_count: 1,
            combine_when_excess_factor: 2.0,
            current_count,
        }
    }

    #[test]
    fn plans_split_when_deficit_exists() {
        let plans = plan_coin_ops(&[bucket(1, 5, 2), bucket(10, 2, 3)], 10, 100, 1, 1);
        assert!(!plans.is_empty());
        assert_eq!(plans[0].op_type, CoinOpKind::Split);
        assert_eq!(plans[0].size_base_units, 1);
    }

    #[test]
    fn plans_combine_only_when_no_deficits() {
        let plans = plan_coin_ops(&[bucket(1, 5, 12)], 4, 10, 1, 1);
        assert!(!plans.is_empty());
        assert_eq!(plans[0].op_type, CoinOpKind::Combine);
    }
}
