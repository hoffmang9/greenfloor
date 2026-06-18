//! Deterministic bootstrap mixed-output planner for offer denomination preflight.
//!
//! `output_amounts_base_units` is the authoritative mixed-split output list for
//! `run_signer_denomination_phase` (passed to vault mixed-split as `output_amounts`).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerLadderRow {
    pub size_base_units: i64,
    pub target_count: i64,
    pub split_buffer_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LadderDeficit {
    pub size_base_units: i64,
    pub required_count: i64,
    pub current_count: i64,
    pub deficit_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapCoin {
    pub id: String,
    pub amount: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapPlan {
    pub source_coin_id: String,
    pub source_amount: i64,
    pub output_amounts_base_units: Vec<i64>,
    pub total_output_amount: i64,
    pub change_amount: i64,
    pub deficits: Vec<LadderDeficit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapPlanOutcome {
    Ready,
    NeedsSplit(BootstrapPlan),
    CannotFund { total_output_amount: i64 },
    InvalidLadder,
    InvalidCoins,
}

fn ladder_row_valid(row: &PlannerLadderRow) -> bool {
    row.size_base_units >= 0 && row.target_count >= 0 && row.split_buffer_count >= 0
}

fn spendable_coins_valid(coins: &[BootstrapCoin]) -> bool {
    coins.iter().all(|coin| coin.amount >= 0)
}

fn sorted_ladder_rows(ladder_entries: &[PlannerLadderRow]) -> Vec<PlannerLadderRow> {
    let mut sorted = ladder_entries.to_vec();
    sorted.sort_by_key(|row| row.size_base_units);
    sorted
}

fn count_exact_amount_coins(
    spendable_coin_amounts: &[i64],
    ladder_sizes: &[i64],
) -> std::collections::HashMap<i64, i64> {
    let ladder: std::collections::HashSet<i64> = ladder_sizes.iter().copied().collect();
    let mut counts: std::collections::HashMap<i64, i64> =
        ladder_sizes.iter().map(|size| (*size, 0)).collect();
    for amount in spendable_coin_amounts {
        if ladder.contains(amount) {
            *counts.get_mut(amount).expect("ladder size pre-seeded") += 1;
        }
    }
    counts
}

/// Build a one-shot mixed-output bootstrap plan from ladder deficits.
pub fn plan_bootstrap_mixed_outputs(
    ladder_entries: &[PlannerLadderRow],
    spendable_coins: &[BootstrapCoin],
) -> BootstrapPlanOutcome {
    if !ladder_entries.iter().all(ladder_row_valid) {
        return BootstrapPlanOutcome::InvalidLadder;
    }
    if !spendable_coins_valid(spendable_coins) {
        return BootstrapPlanOutcome::InvalidCoins;
    }

    let sorted_ladder = sorted_ladder_rows(ladder_entries);
    if sorted_ladder.is_empty() {
        return BootstrapPlanOutcome::InvalidLadder;
    }

    let ladder_sizes: Vec<i64> = sorted_ladder
        .iter()
        .map(|row| row.size_base_units)
        .collect();
    let spendable_amounts: Vec<i64> = spendable_coins.iter().map(|coin| coin.amount).collect();
    let counts = count_exact_amount_coins(&spendable_amounts, &ladder_sizes);

    let mut deficits = Vec::new();
    let mut output_amounts = Vec::new();
    for row in &sorted_ladder {
        let size = row.size_base_units;
        let required = row.target_count + row.split_buffer_count;
        let current = *counts.get(&size).unwrap_or(&0);
        let deficit = required - current;
        if deficit <= 0 {
            continue;
        }
        let deficit_count = usize::try_from(deficit).expect("deficit is positive");
        deficits.push(LadderDeficit {
            size_base_units: size,
            required_count: required,
            current_count: current,
            deficit_count: deficit,
        });
        output_amounts.extend(std::iter::repeat_n(size, deficit_count));
    }

    if deficits.is_empty() {
        return BootstrapPlanOutcome::Ready;
    }

    let total_output_amount: i64 = output_amounts.iter().sum();
    if total_output_amount <= 0 {
        return BootstrapPlanOutcome::InvalidLadder;
    }

    let mut sorted_coins: Vec<&BootstrapCoin> = spendable_coins.iter().collect();
    sorted_coins.sort_by_key(|coin| std::cmp::Reverse(coin.amount));

    let candidate = sorted_coins.into_iter().find_map(|coin| {
        let coin_id = coin.id.trim();
        if coin_id.is_empty() {
            return None;
        }
        if coin.amount >= total_output_amount {
            Some((coin_id.to_string(), coin.amount))
        } else {
            None
        }
    });

    let Some((source_coin_id, source_amount)) = candidate else {
        return BootstrapPlanOutcome::CannotFund {
            total_output_amount,
        };
    };

    BootstrapPlanOutcome::NeedsSplit(BootstrapPlan {
        source_coin_id,
        source_amount,
        output_amounts_base_units: output_amounts,
        total_output_amount,
        change_amount: source_amount - total_output_amount,
        deficits,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        plan_bootstrap_mixed_outputs, BootstrapCoin, BootstrapPlan, BootstrapPlanOutcome,
        LadderDeficit, PlannerLadderRow,
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
            amount,
        }
    }

    #[test]
    fn builds_deficit_outputs() {
        let ladder = vec![row(1, 3, 0), row(10, 2, 1), row(100, 1, 0)];
        let spendable = vec![
            coin("coin-small-1", 1),
            coin("coin-big", 1000),
            coin("coin-hundred", 100),
        ];
        let BootstrapPlanOutcome::NeedsSplit(plan) =
            plan_bootstrap_mixed_outputs(&ladder, &spendable)
        else {
            panic!("expected needs_split")
        };
        assert_eq!(plan.source_coin_id, "coin-big");
        let mut outputs = plan.output_amounts_base_units;
        outputs.sort_unstable();
        assert_eq!(outputs, vec![1, 1, 10, 10, 10]);
        assert_eq!(plan.total_output_amount, 32);
    }

    #[test]
    fn returns_ready_when_inventory_satisfied() {
        let ladder = vec![row(1, 1, 0), row(10, 1, 0)];
        let spendable = vec![
            coin("coin-1", 1),
            coin("coin-10", 10),
            coin("coin-extra", 500),
        ];
        assert_eq!(
            plan_bootstrap_mixed_outputs(&ladder, &spendable),
            BootstrapPlanOutcome::Ready
        );
    }

    #[test]
    fn selects_largest_funding_coin() {
        let ladder = vec![row(10, 2, 0)];
        let spendable = vec![coin("coin-big-object", 100)];
        let BootstrapPlanOutcome::NeedsSplit(plan) =
            plan_bootstrap_mixed_outputs(&ladder, &spendable)
        else {
            panic!("expected needs_split")
        };
        assert_eq!(plan.source_coin_id, "coin-big-object");
        assert_eq!(plan.output_amounts_base_units, vec![10, 10]);
    }

    #[test]
    fn skips_coins_without_id() {
        let ladder = vec![row(10, 2, 0)];
        let spendable = vec![coin("", 1000), coin("valid", 100)];
        let BootstrapPlanOutcome::NeedsSplit(plan) =
            plan_bootstrap_mixed_outputs(&ladder, &spendable)
        else {
            panic!("expected needs_split")
        };
        assert_eq!(plan.source_coin_id, "valid");
    }

    #[test]
    fn returns_cannot_fund_when_no_funding_coin() {
        let ladder = vec![row(10, 2, 0)];
        let spendable = vec![coin("small", 5)];
        assert_eq!(
            plan_bootstrap_mixed_outputs(&ladder, &spendable),
            BootstrapPlanOutcome::CannotFund {
                total_output_amount: 20
            }
        );
    }

    #[test]
    fn preserves_deficit_metadata() {
        let ladder = vec![row(10, 2, 1)];
        let spendable = vec![coin("coin-big", 1000)];
        let BootstrapPlanOutcome::NeedsSplit(plan) =
            plan_bootstrap_mixed_outputs(&ladder, &spendable)
        else {
            panic!("expected needs_split")
        };
        assert_eq!(
            plan.deficits,
            vec![LadderDeficit {
                size_base_units: 10,
                required_count: 3,
                current_count: 0,
                deficit_count: 3,
            }]
        );
    }

    #[test]
    fn empty_ladder_is_invalid() {
        assert_eq!(
            plan_bootstrap_mixed_outputs(&[], &[coin("x", 1)]),
            BootstrapPlanOutcome::InvalidLadder
        );
    }

    #[test]
    fn single_output_plan_when_only_one_deficit_coin_needed() {
        let ladder = vec![row(10, 1, 0)];
        let spendable = vec![coin("coin-big", 100)];
        let BootstrapPlanOutcome::NeedsSplit(plan) =
            plan_bootstrap_mixed_outputs(&ladder, &spendable)
        else {
            panic!("expected needs_split")
        };
        assert_eq!(plan.output_amounts_base_units, vec![10]);
        assert_eq!(plan.total_output_amount, 10);
    }

    #[test]
    fn returns_invalid_ladder_for_negative_fields() {
        assert_eq!(
            plan_bootstrap_mixed_outputs(&[row(-1, 1, 0)], &[coin("x", 100)]),
            BootstrapPlanOutcome::InvalidLadder
        );
        assert_eq!(
            plan_bootstrap_mixed_outputs(&[row(10, -1, 0)], &[coin("x", 100)]),
            BootstrapPlanOutcome::InvalidLadder
        );
        assert_eq!(
            plan_bootstrap_mixed_outputs(&[row(10, 1, -1)], &[coin("x", 100)]),
            BootstrapPlanOutcome::InvalidLadder
        );
    }

    #[test]
    fn returns_invalid_coins_for_negative_amount() {
        let ladder = vec![row(10, 1, 0)];
        assert_eq!(
            plan_bootstrap_mixed_outputs(&ladder, &[coin("bad", -5)]),
            BootstrapPlanOutcome::InvalidCoins
        );
    }

    #[test]
    fn change_amount_matches_source_minus_outputs() {
        let ladder = vec![row(10, 2, 0)];
        let spendable = vec![coin("coin-big", 100)];
        let BootstrapPlanOutcome::NeedsSplit(BootstrapPlan {
            source_amount,
            total_output_amount,
            change_amount,
            ..
        }) = plan_bootstrap_mixed_outputs(&ladder, &spendable)
        else {
            panic!("expected needs_split")
        };
        assert_eq!(change_amount, source_amount - total_output_amount);
    }
}
