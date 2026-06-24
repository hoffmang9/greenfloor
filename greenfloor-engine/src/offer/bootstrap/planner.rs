//! Deterministic bootstrap mixed-output planner for offer denomination preflight.
//!
//! `output_amounts_base_units` is the authoritative mixed-split output list for
//! `run_signer_denomination_phase` (passed to vault mixed-split as `output_amounts`).

use crate::coin_ops::aggregate_covers_without_single_coin;

use super::amounts::BaseUnits;
use super::combine_inputs::BootstrapCombineInputs;
use super::combine_plan::{build_bootstrap_combine_plan, BootstrapCombineContext};
use super::ladder::{
    ladder_shape_context_for_bootstrap, select_smallest_non_cannibalizing_bootstrap_coin,
};

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
    pub amount: BaseUnits,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapFundingSource {
    SingleCoin { coin_id: String, amount: BaseUnits },
    CombineFirst(BootstrapCombineInputs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapPlan {
    pub funding: BootstrapFundingSource,
    pub output_amounts_base_units: Vec<i64>,
    pub total_output_amount: i64,
    /// Leftover base units after shaping (not mojos). Convert before CAT dust checks.
    pub change_amount: i64,
    pub deficits: Vec<LadderDeficit>,
}

impl BootstrapPlan {
    #[must_use]
    pub fn requires_combine_first(&self) -> bool {
        matches!(self.funding, BootstrapFundingSource::CombineFirst(_))
    }

    #[must_use]
    pub fn source_coin_id(&self) -> Option<&str> {
        match &self.funding {
            BootstrapFundingSource::SingleCoin { coin_id, .. } => Some(coin_id.as_str()),
            BootstrapFundingSource::CombineFirst(_) => None,
        }
    }

    #[must_use]
    pub fn source_amount(&self) -> i64 {
        match &self.funding {
            BootstrapFundingSource::SingleCoin { amount, .. } => amount.get(),
            BootstrapFundingSource::CombineFirst(inputs) => inputs.selected_total.get(),
        }
    }

    #[must_use]
    pub fn combine_inputs(&self) -> Option<&BootstrapCombineInputs> {
        match &self.funding {
            BootstrapFundingSource::CombineFirst(inputs) => Some(inputs),
            BootstrapFundingSource::SingleCoin { .. } => None,
        }
    }

    #[must_use]
    pub fn combine_input_coin_ids(&self) -> Option<&[String]> {
        self.combine_inputs()
            .map(|inputs| inputs.input_coin_ids.as_slice())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapPlanOutcome {
    Ready,
    NeedsShape(BootstrapPlan),
    CannotFund { total_output_amount: i64 },
    InvalidLadder,
    InvalidCoins,
}

fn spendable_for_combine(coins: &[BootstrapCoin]) -> Vec<BootstrapCoin> {
    coins
        .iter()
        .filter(|coin| !coin.id.trim().is_empty() && coin.amount.get() > 0)
        .cloned()
        .collect()
}

fn ladder_row_valid(row: &PlannerLadderRow) -> bool {
    row.size_base_units >= 0 && row.target_count >= 0 && row.split_buffer_count >= 0
}

fn spendable_coins_valid(coins: &[BootstrapCoin]) -> bool {
    coins.iter().all(|coin| coin.amount.get() >= 0)
}

fn sorted_ladder_rows(ladder_entries: &[PlannerLadderRow]) -> Vec<PlannerLadderRow> {
    let mut sorted = ladder_entries.to_vec();
    sorted.sort_by_key(|row| row.size_base_units);
    sorted
}

/// Build a one-shot mixed-output bootstrap plan from ladder deficits.
///
/// # Panics
///
/// Panics if a positive deficit does not fit in `usize` (internal invariant).
pub fn plan_bootstrap_mixed_outputs(
    ladder_entries: &[PlannerLadderRow],
    spendable_coins: &[BootstrapCoin],
    combine_input_cap: i64,
    combine_context: &BootstrapCombineContext,
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

    let spendable_amounts: Vec<i64> = spendable_coins
        .iter()
        .map(|coin| coin.amount.get())
        .collect();
    let shape_ctx = ladder_shape_context_for_bootstrap(&sorted_ladder, &spendable_amounts);
    let protected_slots = &shape_ctx.protected_slots;
    let counts = &shape_ctx.exact_ladder_counts;

    let mut deficits = Vec::new();
    let mut output_amounts = Vec::new();
    for row in &sorted_ladder {
        let size = row.size_base_units;
        let required = protected_slots.get(&size).copied().unwrap_or(0);
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

    let candidate = select_smallest_non_cannibalizing_bootstrap_coin(
        spendable_coins,
        total_output_amount,
        &shape_ctx,
    )
    .map(|coin| (coin.id.clone(), coin.amount));

    let Some((source_coin_id, source_amount)) = candidate else {
        if !aggregate_covers_without_single_coin(total_output_amount, &spendable_amounts) {
            return BootstrapPlanOutcome::CannotFund {
                total_output_amount,
            };
        }
        let spendable_for_combine = spendable_for_combine(spendable_coins);
        let Some(combine_inputs) = build_bootstrap_combine_plan(
            &spendable_for_combine,
            &sorted_ladder,
            BaseUnits::new(total_output_amount),
            combine_input_cap,
            combine_context,
        ) else {
            return BootstrapPlanOutcome::CannotFund {
                total_output_amount,
            };
        };
        let selected_total = combine_inputs.selected_total.get();
        return BootstrapPlanOutcome::NeedsShape(BootstrapPlan {
            funding: BootstrapFundingSource::CombineFirst(combine_inputs),
            output_amounts_base_units: output_amounts,
            total_output_amount,
            change_amount: selected_total - total_output_amount,
            deficits,
        });
    };

    BootstrapPlanOutcome::NeedsShape(BootstrapPlan {
        funding: BootstrapFundingSource::SingleCoin {
            coin_id: source_coin_id,
            amount: source_amount,
        },
        output_amounts_base_units: output_amounts,
        total_output_amount,
        change_amount: source_amount.get() - total_output_amount,
        deficits,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        plan_bootstrap_mixed_outputs, BootstrapCoin, BootstrapFundingSource, BootstrapPlan,
        BootstrapPlanOutcome, LadderDeficit, PlannerLadderRow,
    };
    use crate::coin_ops::aggregate_covers_without_single_coin;
    use crate::offer::bootstrap::{BaseUnits, BootstrapCombineContext};

    const TEST_COMBINE_CAP: i64 = 5;

    fn test_combine_context() -> BootstrapCombineContext {
        BootstrapCombineContext::for_tests()
    }

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

    fn assert_single_coin_split(plan: &BootstrapPlan) {
        assert!(matches!(
            plan.funding,
            BootstrapFundingSource::SingleCoin { .. }
        ));
    }

    fn assert_combine_first(plan: &BootstrapPlan) {
        assert!(plan.requires_combine_first());
    }

    #[test]
    fn builds_deficit_outputs() {
        let ladder = vec![row(1, 3, 0), row(10, 2, 1), row(100, 1, 0)];
        let spendable = vec![
            coin("coin-small-1", 1),
            coin("coin-big", 1000),
            coin("coin-hundred", 100),
        ];
        let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
            &ladder,
            &spendable,
            TEST_COMBINE_CAP,
            &test_combine_context(),
        ) else {
            panic!("expected needs_shape")
        };
        assert_single_coin_split(&plan);
        assert_eq!(plan.source_coin_id(), Some("coin-big"));
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
            plan_bootstrap_mixed_outputs(
                &ladder,
                &spendable,
                TEST_COMBINE_CAP,
                &test_combine_context()
            ),
            BootstrapPlanOutcome::Ready
        );
    }

    #[test]
    fn selects_smallest_non_cannibalizing_funding_coin() {
        let ladder = vec![row(10, 2, 0)];
        let spendable = vec![coin("coin-big-object", 100)];
        let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
            &ladder,
            &spendable,
            TEST_COMBINE_CAP,
            &test_combine_context(),
        ) else {
            panic!("expected needs_shape")
        };
        assert_eq!(plan.source_coin_id(), Some("coin-big-object"));
        assert_eq!(plan.output_amounts_base_units, vec![10, 10]);
    }

    #[test]
    fn skips_satisfied_ladder_row_when_smaller_non_ladder_coin_exists() {
        let ladder = vec![row(10, 2, 1), row(100, 1, 0)];
        let spendable = vec![coin("combined", 100), coin("spare", 50), coin("ten", 10)];
        let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
            &ladder,
            &spendable,
            TEST_COMBINE_CAP,
            &test_combine_context(),
        ) else {
            panic!("expected needs_shape")
        };
        assert_eq!(plan.source_coin_id(), Some("spare"));
        assert_eq!(plan.total_output_amount, 20);
    }

    #[test]
    fn skips_coins_without_id() {
        let ladder = vec![row(10, 2, 0)];
        let spendable = vec![coin("", 1000), coin("valid", 100)];
        let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
            &ladder,
            &spendable,
            TEST_COMBINE_CAP,
            &test_combine_context(),
        ) else {
            panic!("expected needs_shape")
        };
        assert_eq!(plan.source_coin_id(), Some("valid"));
    }

    #[test]
    fn returns_cannot_fund_when_no_funding_coin() {
        let ladder = vec![row(10, 2, 0)];
        let spendable = vec![coin("small", 5)];
        assert_eq!(
            plan_bootstrap_mixed_outputs(
                &ladder,
                &spendable,
                TEST_COMBINE_CAP,
                &test_combine_context()
            ),
            BootstrapPlanOutcome::CannotFund {
                total_output_amount: 20
            }
        );
    }

    #[test]
    fn preserves_deficit_metadata() {
        let ladder = vec![row(10, 2, 1)];
        let spendable = vec![coin("coin-big", 1000)];
        let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
            &ladder,
            &spendable,
            TEST_COMBINE_CAP,
            &test_combine_context(),
        ) else {
            panic!("expected needs_shape")
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
            plan_bootstrap_mixed_outputs(
                &[],
                &[coin("x", 1)],
                TEST_COMBINE_CAP,
                &test_combine_context()
            ),
            BootstrapPlanOutcome::InvalidLadder
        );
    }

    #[test]
    fn single_output_plan_when_only_one_deficit_coin_needed() {
        let ladder = vec![row(10, 1, 0)];
        let spendable = vec![coin("coin-big", 100)];
        let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
            &ladder,
            &spendable,
            TEST_COMBINE_CAP,
            &test_combine_context(),
        ) else {
            panic!("expected needs_shape")
        };
        assert_eq!(plan.output_amounts_base_units, vec![10]);
        assert_eq!(plan.total_output_amount, 10);
    }

    #[test]
    fn returns_invalid_ladder_for_negative_fields() {
        assert_eq!(
            plan_bootstrap_mixed_outputs(
                &[row(-1, 1, 0)],
                &[coin("x", 100)],
                TEST_COMBINE_CAP,
                &test_combine_context()
            ),
            BootstrapPlanOutcome::InvalidLadder
        );
        assert_eq!(
            plan_bootstrap_mixed_outputs(
                &[row(10, -1, 0)],
                &[coin("x", 100)],
                TEST_COMBINE_CAP,
                &test_combine_context()
            ),
            BootstrapPlanOutcome::InvalidLadder
        );
        assert_eq!(
            plan_bootstrap_mixed_outputs(
                &[row(10, 1, -1)],
                &[coin("x", 100)],
                TEST_COMBINE_CAP,
                &test_combine_context()
            ),
            BootstrapPlanOutcome::InvalidLadder
        );
    }

    #[test]
    fn returns_invalid_coins_for_negative_amount() {
        let ladder = vec![row(10, 1, 0)];
        assert_eq!(
            plan_bootstrap_mixed_outputs(
                &ladder,
                &[coin("bad", -5)],
                TEST_COMBINE_CAP,
                &test_combine_context()
            ),
            BootstrapPlanOutcome::InvalidCoins
        );
    }

    #[test]
    fn change_amount_matches_source_minus_outputs() {
        let ladder = vec![row(10, 2, 0)];
        let spendable = vec![coin("coin-big", 100)];
        let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
            &ladder,
            &spendable,
            TEST_COMBINE_CAP,
            &test_combine_context(),
        ) else {
            panic!("expected needs_shape")
        };
        assert_eq!(
            plan.change_amount,
            plan.source_amount() - plan.total_output_amount
        );
    }

    #[test]
    fn plans_combine_first_when_aggregate_covers_deficit_without_single_coin() {
        let ladder = vec![row(100, 1, 0)];
        let spendable = vec![
            coin("sixty-five", 65),
            coin("twenty", 20),
            coin("eleven", 11),
            coin("four", 4),
        ];
        let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
            &ladder,
            &spendable,
            TEST_COMBINE_CAP,
            &test_combine_context(),
        ) else {
            panic!("expected needs_shape combine-first")
        };
        assert_combine_first(&plan);
        assert_eq!(plan.total_output_amount, 100);
        assert_eq!(plan.output_amounts_base_units, vec![100]);
        let input_ids = plan.combine_input_coin_ids().expect("combine input ids");
        assert!(input_ids.len() >= 2);
        assert!(plan.source_amount() >= 100);
    }

    #[test]
    fn aggregate_covers_without_single_coin_matches_defer_inventory() {
        assert!(aggregate_covers_without_single_coin(100, &[65, 20, 11, 4]));
        assert!(!aggregate_covers_without_single_coin(100, &[150, 10]));
    }

    #[test]
    fn capped_combine_returns_cannot_fund_when_truncated_inputs_are_insufficient() {
        let ladder = vec![row(100, 1, 0)];
        let spendable = vec![
            coin("sixty-five", 65),
            coin("twenty", 20),
            coin("eleven", 11),
            coin("four", 4),
        ];
        assert_eq!(
            plan_bootstrap_mixed_outputs(&ladder, &spendable, 2, &test_combine_context()),
            BootstrapPlanOutcome::CannotFund {
                total_output_amount: 100
            }
        );
        assert_eq!(
            plan_bootstrap_mixed_outputs(&ladder, &spendable, 3, &test_combine_context()),
            BootstrapPlanOutcome::CannotFund {
                total_output_amount: 100
            }
        );
    }

    #[test]
    fn capped_combine_succeeds_when_cap_includes_enough_inputs() {
        let ladder = vec![row(100, 1, 0)];
        let spendable = vec![
            coin("sixty-five", 65),
            coin("twenty", 20),
            coin("eleven", 11),
            coin("four", 4),
        ];
        let BootstrapPlanOutcome::NeedsShape(plan) =
            plan_bootstrap_mixed_outputs(&ladder, &spendable, 4, &test_combine_context())
        else {
            panic!("expected needs_shape with cap=4")
        };
        assert_combine_first(&plan);
        assert_eq!(plan.combine_input_coin_ids().expect("inputs").len(), 4);
    }

    #[test]
    fn plans_combine_first_for_fragmented_inventory_with_cap_five() {
        let ladder = vec![row(1, 5, 1), row(10, 2, 1), row(100, 1, 0)];
        let spendable: Vec<BootstrapCoin> =
            crate::test_support::fragmented_combine_cap_inventory::fragmented_combine_cap_spendable_coins()
                .into_iter()
                .map(|coin_row| coin(&coin_row.id, coin_row.amount))
                .collect();
        let BootstrapPlanOutcome::NeedsShape(plan) =
            plan_bootstrap_mixed_outputs(&ladder, &spendable, 5, &test_combine_context())
        else {
            panic!("expected combine-first plan for fragmented inventory")
        };
        assert_combine_first(&plan);
        assert_eq!(plan.total_output_amount, 100);
        let combine = plan.combine_inputs().expect("combine inputs");
        assert_eq!(combine.target_amount, BaseUnits::new(100));
        assert!(combine.selected_total.get() >= 100);
        assert_eq!(plan.change_amount, combine.selected_total.get() - 100);
        let inputs = plan.combine_input_coin_ids().expect("combine input ids");
        assert!(inputs.len() >= 2);
        assert!(inputs.len() <= 5);
        assert!(plan.source_amount() >= 100);
    }

    #[test]
    fn eco181_inventory_replan_after_combine_preserves_hundred_row() {
        use crate::test_support::eco181_bootstrap_inventory::{
            eco181_after_combine_coins, eco181_bootstrap_coins, eco181_bootstrap_ladder,
        };

        let ladder = eco181_bootstrap_ladder();
        let BootstrapPlanOutcome::NeedsShape(plan) = plan_bootstrap_mixed_outputs(
            &ladder,
            &eco181_bootstrap_coins(),
            TEST_COMBINE_CAP,
            &test_combine_context(),
        ) else {
            panic!("expected combine-first plan")
        };
        assert_eq!(plan.total_output_amount, 100);

        let remaining = plan_bootstrap_mixed_outputs(
            &ladder,
            &eco181_after_combine_coins(),
            TEST_COMBINE_CAP,
            &test_combine_context(),
        );
        match remaining {
            BootstrapPlanOutcome::Ready => {}
            BootstrapPlanOutcome::CannotFund {
                total_output_amount,
            } => {
                assert!(
                    total_output_amount < 100,
                    "100 BU row must stay satisfied after combine: {remaining:?}"
                );
            }
            BootstrapPlanOutcome::NeedsShape(ref split) => {
                assert_ne!(
                    split.source_amount(),
                    100,
                    "must not split the satisfied 100 BU row for smaller deficits: {remaining:?}"
                );
                assert!(
                    !split
                        .deficits
                        .iter()
                        .any(|deficit| deficit.size_base_units == 100),
                    "100 BU row must stay satisfied after combine: {remaining:?}"
                );
            }
            other => panic!("unexpected post-combine outcome: {other:?}"),
        }
    }
}
