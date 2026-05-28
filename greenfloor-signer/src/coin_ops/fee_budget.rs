use super::plan::{CoinOpKind, CoinOpPlan};

pub fn projected_coin_ops_fee_mojos(
    plans: &[CoinOpPlan],
    split_fee_mojos: i64,
    combine_fee_mojos: i64,
) -> i64 {
    let mut total = 0_i64;
    for plan in plans {
        let per_op_fee = plan.op_type.fee_mojos(split_fee_mojos, combine_fee_mojos);
        total += plan.op_count.max(0) * per_op_fee.max(0);
    }
    total
}

pub fn fee_budget_allows_execution(
    max_daily_fee_budget_mojos: i64,
    spent_today_mojos: i64,
    projected_mojos: i64,
) -> bool {
    if max_daily_fee_budget_mojos <= 0 {
        return true;
    }
    spent_today_mojos + projected_mojos <= max_daily_fee_budget_mojos
}

pub fn partition_plans_by_budget(
    plans: &[CoinOpPlan],
    split_fee_mojos: i64,
    combine_fee_mojos: i64,
    spent_today_mojos: i64,
    max_daily_fee_budget_mojos: i64,
) -> (Vec<CoinOpPlan>, Vec<CoinOpPlan>) {
    if max_daily_fee_budget_mojos <= 0 {
        return (plans.to_vec(), Vec::new());
    }

    let mut remaining = (max_daily_fee_budget_mojos - spent_today_mojos.max(0)).max(0);
    let mut allowed = Vec::new();
    let mut skipped = Vec::new();

    for plan in plans {
        let per_op = plan
            .op_type
            .fee_mojos(split_fee_mojos, combine_fee_mojos)
            .max(0);
        if plan.op_count <= 0 {
            continue;
        }
        if per_op == 0 {
            allowed.push(plan.clone());
            continue;
        }
        let affordable_ops = remaining / per_op;
        if affordable_ops <= 0 {
            skipped.push(plan.clone());
            continue;
        }
        if affordable_ops >= plan.op_count {
            allowed.push(plan.clone());
            remaining -= plan.op_count * per_op;
            continue;
        }
        allowed.push(CoinOpPlan {
            op_type: plan.op_type,
            size_base_units: plan.size_base_units,
            op_count: affordable_ops,
            reason: plan.reason.clone(),
        });
        skipped.push(CoinOpPlan {
            op_type: plan.op_type,
            size_base_units: plan.size_base_units,
            op_count: plan.op_count - affordable_ops,
            reason: "fee_budget_partial_overflow".to_string(),
        });
        remaining = 0;
    }

    (allowed, skipped)
}

#[cfg(test)]
mod tests {
    use super::{
        fee_budget_allows_execution, partition_plans_by_budget, projected_coin_ops_fee_mojos,
    };
    use crate::coin_ops::plan::{CoinOpKind, CoinOpPlan};

    #[test]
    fn projected_fee_sums_per_op_type() {
        let fee = projected_coin_ops_fee_mojos(
            &[
                CoinOpPlan {
                    op_type: CoinOpKind::Split,
                    size_base_units: 1,
                    op_count: 3,
                    reason: "x".to_string(),
                },
                CoinOpPlan {
                    op_type: CoinOpKind::Combine,
                    size_base_units: 10,
                    op_count: 2,
                    reason: "y".to_string(),
                },
            ],
            5,
            7,
        );
        assert_eq!(fee, (3 * 5) + (2 * 7));
    }

    #[test]
    fn fee_budget_guard() {
        assert!(fee_budget_allows_execution(100, 40, 50));
        assert!(!fee_budget_allows_execution(100, 60, 50));
    }

    #[test]
    fn partition_partial_split() {
        let (allowed, skipped) = partition_plans_by_budget(
            &[CoinOpPlan {
                op_type: CoinOpKind::Split,
                size_base_units: 1,
                op_count: 5,
                reason: "r".to_string(),
            }],
            10,
            10,
            25,
            55,
        );
        assert_eq!(allowed.len(), 1);
        assert_eq!(allowed[0].op_count, 3);
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].op_count, 2);
    }
}
