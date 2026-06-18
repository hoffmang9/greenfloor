//! Auto split/combine input planning (CLI vs daemon profiles).

use std::collections::{HashMap, HashSet};

use super::policy::coin_op_min_amount_mojos;
use super::selection::{
    select_exact_amount_coin_ids, select_spendable_coins_for_target_amount,
    split_would_create_sub_cat_change, SpendableCoin,
};
use crate::metrics::non_negative_i64_to_usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitPlanningProfile {
    CliAuto,
    DaemonAuto,
}

impl SplitPlanningProfile {
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "cli_auto" => Some(Self::CliAuto),
            "daemon_auto" => Some(Self::DaemonAuto),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombineInputSelectionMode {
    LargestByAmount,
    ExactAmount,
}

impl CombineInputSelectionMode {
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "largest_by_amount" => Some(Self::LargestByAmount),
            "exact_amount" => Some(Self::ExactAmount),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitCombinePrereqPlan {
    pub input_coin_ids: Vec<String>,
    pub target_amount: i64,
    pub selected_total: i64,
    pub exact_match: bool,
    pub cap_applied: bool,
    pub selected_count_before_cap: usize,
    pub combine_input_cap: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitCoinPlan {
    pub coin_id: String,
    pub selected_amount_mojos: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubCatChangeSkipData {
    pub selected_coin_id: String,
    pub selected_amount_mojos: i64,
    pub required_amount_mojos: i64,
    pub remainder_mojos: i64,
    pub minimum_allowed_mojos: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitSkipPlan {
    pub reason: String,
    pub data: Option<SubCatChangeSkipData>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplitAutoSelectPlan {
    Coin(SplitCoinPlan),
    CombinePrereq(SplitCombinePrereqPlan),
    Skip(SplitSkipPlan),
}

struct SplitPlanningBehavior {
    enforce_required_amount: bool,
    check_sub_cat_change: bool,
    default_allow_combine_prereq: bool,
}

fn behavior_for(profile: SplitPlanningProfile) -> SplitPlanningBehavior {
    match profile {
        SplitPlanningProfile::CliAuto => SplitPlanningBehavior {
            enforce_required_amount: false,
            check_sub_cat_change: false,
            default_allow_combine_prereq: false,
        },
        SplitPlanningProfile::DaemonAuto => SplitPlanningBehavior {
            enforce_required_amount: true,
            check_sub_cat_change: true,
            default_allow_combine_prereq: true,
        },
    }
}

pub fn build_combine_prereq_plan(
    candidate_spendable: &[SpendableCoin],
    required_amount_mojos: i64,
    combine_input_cap: i64,
) -> Option<SplitCombinePrereqPlan> {
    let (combine_coin_ids, _combine_total, _exact_match) =
        select_spendable_coins_for_target_amount(candidate_spendable, required_amount_mojos);
    if combine_coin_ids.len() < 2 {
        return None;
    }
    let amount_by_coin_id: HashMap<String, i64> = candidate_spendable
        .iter()
        .map(|coin| (coin.id.clone(), coin.amount))
        .collect();
    let cap = non_negative_i64_to_usize(combine_input_cap);
    let combine_input_coin_ids: Vec<String> = combine_coin_ids.iter().take(cap).cloned().collect();
    let combine_cap_applied = combine_input_coin_ids.len() < combine_coin_ids.len();
    let combine_selected_total: i64 = combine_input_coin_ids
        .iter()
        .map(|id| amount_by_coin_id.get(id).copied().unwrap_or(0))
        .sum();
    let combine_exact_match = combine_selected_total == required_amount_mojos;
    let combine_target_amount = if combine_selected_total >= required_amount_mojos {
        required_amount_mojos
    } else {
        combine_selected_total
    };
    Some(SplitCombinePrereqPlan {
        input_coin_ids: combine_input_coin_ids,
        target_amount: combine_target_amount,
        selected_total: combine_selected_total,
        exact_match: combine_exact_match,
        cap_applied: combine_cap_applied,
        selected_count_before_cap: combine_coin_ids.len(),
        combine_input_cap,
    })
}

pub fn plan_auto_split_selection(
    candidate_spendable: &[SpendableCoin],
    required_amount_mojos: i64,
    canonical_asset_id: &str,
    profile: SplitPlanningProfile,
    combine_input_cap: i64,
    allow_combine_prereq: Option<bool>,
) -> SplitAutoSelectPlan {
    let behavior = behavior_for(profile);
    let resolve_allow_combine_prereq =
        allow_combine_prereq.unwrap_or(behavior.default_allow_combine_prereq);
    let enforce_required_amount = behavior.enforce_required_amount;
    let check_sub_cat_change = behavior.check_sub_cat_change;

    let large_enough: Vec<&SpendableCoin> = if enforce_required_amount && required_amount_mojos > 0
    {
        candidate_spendable
            .iter()
            .filter(|coin| coin.amount >= required_amount_mojos)
            .collect()
    } else {
        candidate_spendable.iter().collect()
    };

    if !large_enough.is_empty() {
        let selected_coin = large_enough
            .iter()
            .filter(|coin| !coin.id.is_empty())
            .max_by_key(|coin| coin.amount);
        if selected_coin.is_none() {
            return SplitAutoSelectPlan::Skip(SplitSkipPlan {
                reason: "no_spendable_split_coin_meets_required_amount".to_string(),
                data: None,
            });
        }
        let selected_coin = selected_coin.expect("checked");
        if selected_coin.id.is_empty() {
            return SplitAutoSelectPlan::Skip(SplitSkipPlan {
                reason: "no_spendable_split_coin_meets_required_amount".to_string(),
                data: None,
            });
        }
        let selected_amount = selected_coin.amount;
        if check_sub_cat_change && enforce_required_amount {
            let (would_create_dust, remainder) = split_would_create_sub_cat_change(
                selected_amount,
                required_amount_mojos,
                canonical_asset_id,
            );
            if would_create_dust {
                return SplitAutoSelectPlan::Skip(SplitSkipPlan {
                    reason: "split_would_create_sub_cat_change".to_string(),
                    data: Some(SubCatChangeSkipData {
                        selected_coin_id: selected_coin.id.clone(),
                        selected_amount_mojos: selected_amount,
                        required_amount_mojos,
                        remainder_mojos: remainder,
                        minimum_allowed_mojos: coin_op_min_amount_mojos(canonical_asset_id),
                    }),
                });
            }
        }
        return SplitAutoSelectPlan::Coin(SplitCoinPlan {
            coin_id: selected_coin.id.clone(),
            selected_amount_mojos: selected_amount,
        });
    }

    let aggregate: i64 = candidate_spendable.iter().map(|c| c.amount).sum();
    if resolve_allow_combine_prereq
        && enforce_required_amount
        && required_amount_mojos > 0
        && aggregate >= required_amount_mojos
    {
        if let Some(prereq) = build_combine_prereq_plan(
            candidate_spendable,
            required_amount_mojos,
            combine_input_cap,
        ) {
            return SplitAutoSelectPlan::CombinePrereq(prereq);
        }
    }

    SplitAutoSelectPlan::Skip(SplitSkipPlan {
        reason: "no_spendable_split_coin_meets_required_amount".to_string(),
        data: None,
    })
}

pub fn plan_auto_combine_inputs(
    spendable_coins: &[SpendableCoin],
    number_of_coins: usize,
    selection_mode: CombineInputSelectionMode,
    target_amount_mojos: Option<i64>,
    exclude_coin_ids: Option<&HashSet<String>>,
    max_count: Option<usize>,
) -> Result<Vec<String>, &'static str> {
    let capped_count = match max_count {
        Some(max) => number_of_coins.min(max),
        None => number_of_coins,
    };
    if selection_mode == CombineInputSelectionMode::ExactAmount {
        let amount = target_amount_mojos
            .ok_or("target_amount_mojos is required for exact-amount combine selection")?;
        let excluded: HashSet<String> = exclude_coin_ids
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default();
        return Ok(select_exact_amount_coin_ids(
            spendable_coins,
            amount,
            &excluded,
            Some(capped_count),
        ));
    }

    let excluded: HashSet<String> = exclude_coin_ids
        .map(|set| set.iter().map(|id| id.to_ascii_lowercase()).collect())
        .unwrap_or_default();
    let mut eligible: Vec<&SpendableCoin> = spendable_coins
        .iter()
        .filter(|coin| !coin.id.is_empty() && !excluded.contains(&coin.id.to_ascii_lowercase()))
        .collect();
    eligible.sort_by(|left, right| right.amount.cmp(&left.amount));
    Ok(eligible
        .iter()
        .take(capped_count)
        .map(|coin| coin.id.clone())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coins(rows: &[(&str, i64)]) -> Vec<SpendableCoin> {
        rows.iter()
            .map(|(id, amount)| SpendableCoin {
                id: (*id).to_string(),
                amount: *amount,
            })
            .collect()
    }

    #[test]
    fn cli_auto_picks_largest_without_required_enforcement() {
        let plan = plan_auto_split_selection(
            &coins(&[("Coin_small", 100), ("Coin_big", 1500)]),
            1000,
            "xch",
            SplitPlanningProfile::CliAuto,
            0,
            None,
        );
        match plan {
            SplitAutoSelectPlan::Coin(coin) => assert_eq!(coin.coin_id, "Coin_big"),
            other => panic!("unexpected plan: {other:?}"),
        }
    }

    #[test]
    fn daemon_auto_requires_single_coin_at_least_required() {
        let plan = plan_auto_split_selection(
            &coins(&[("Coin_small", 500), ("Coin_big", 1500)]),
            1000,
            "xch",
            SplitPlanningProfile::DaemonAuto,
            10,
            Some(false),
        );
        match plan {
            SplitAutoSelectPlan::Coin(coin) => assert_eq!(coin.coin_id, "Coin_big"),
            other => panic!("unexpected plan: {other:?}"),
        }
    }

    #[test]
    fn daemon_auto_skips_when_no_coin_meets_required() {
        let plan = plan_auto_split_selection(
            &coins(&[("Coin_a", 400), ("Coin_b", 500)]),
            1000,
            "xch",
            SplitPlanningProfile::DaemonAuto,
            10,
            Some(false),
        );
        match plan {
            SplitAutoSelectPlan::Skip(skip) => {
                assert_eq!(skip.reason, "no_spendable_split_coin_meets_required_amount");
            }
            other => panic!("unexpected plan: {other:?}"),
        }
    }

    #[test]
    fn daemon_auto_returns_combine_prereq_when_aggregate_covers() {
        let plan = plan_auto_split_selection(
            &coins(&[("Coin_a", 4000), ("Coin_b", 6000)]),
            10_000,
            "xch",
            SplitPlanningProfile::DaemonAuto,
            10,
            Some(true),
        );
        match plan {
            SplitAutoSelectPlan::CombinePrereq(prereq) => {
                assert_eq!(
                    prereq
                        .input_coin_ids
                        .iter()
                        .cloned()
                        .collect::<HashSet<_>>(),
                    HashSet::from(["Coin_a".to_string(), "Coin_b".to_string()])
                );
                assert!(prereq.exact_match);
            }
            other => panic!("unexpected plan: {other:?}"),
        }
    }

    #[test]
    fn cli_auto_does_not_return_combine_prereq() {
        let plan = plan_auto_split_selection(
            &coins(&[("Coin_a", 4000), ("Coin_b", 6000)]),
            10_000,
            "xch",
            SplitPlanningProfile::CliAuto,
            10,
            None,
        );
        match plan {
            SplitAutoSelectPlan::Coin(coin) => assert_eq!(coin.coin_id, "Coin_b"),
            other => panic!("unexpected plan: {other:?}"),
        }
    }

    #[test]
    fn daemon_retry_second_attempt_disables_combine_prereq() {
        let spendable = coins(&[("Coin_a", 4000), ("Coin_b", 6000)]);
        let first = plan_auto_split_selection(
            &spendable,
            10_000,
            "xch",
            SplitPlanningProfile::DaemonAuto,
            10,
            Some(true),
        );
        match first {
            SplitAutoSelectPlan::CombinePrereq(_) => {}
            other => panic!("expected combine prereq on first attempt: {other:?}"),
        }
        let second = plan_auto_split_selection(
            &spendable,
            10_000,
            "xch",
            SplitPlanningProfile::DaemonAuto,
            10,
            Some(false),
        );
        match second {
            SplitAutoSelectPlan::Skip(skip) => {
                assert_eq!(skip.reason, "no_spendable_split_coin_meets_required_amount");
            }
            other => panic!("expected skip when combine prereq disabled: {other:?}"),
        }
    }

    #[test]
    fn combine_exact_amount_skips_non_matching_denominations() {
        let spendable = coins(&[
            ("dust", 500),
            ("coin_a", 1000),
            ("coin_b", 1000),
            ("coin_c", 1000),
        ]);
        let ids = plan_auto_combine_inputs(
            &spendable,
            3,
            CombineInputSelectionMode::ExactAmount,
            Some(1000),
            None::<&HashSet<String>>,
            Some(3),
        )
        .expect("combine inputs");
        assert_eq!(ids, vec!["coin_a", "coin_b", "coin_c"]);
    }

    #[test]
    fn daemon_auto_rejects_sub_cat_change_dust() {
        let cat_id = "0000000000000000000000000000000000000000000000000000000000000001";
        let plan = plan_auto_split_selection(
            &coins(&[("Coin_cat", 10_500)]),
            10_000,
            cat_id,
            SplitPlanningProfile::DaemonAuto,
            10,
            Some(false),
        );
        match plan {
            SplitAutoSelectPlan::Skip(skip) => {
                assert_eq!(skip.reason, "split_would_create_sub_cat_change");
                let data = skip.data.expect("data");
                assert_eq!(data.remainder_mojos, 500);
            }
            other => panic!("unexpected plan: {other:?}"),
        }
    }
}
