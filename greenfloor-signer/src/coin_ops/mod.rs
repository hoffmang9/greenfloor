//! Deterministic coin-operation policy (planning, fee budget, inventory buckets).
//!
//! Lives in the `greenfloor-signer` crate alongside vault signing and cycle policy.
//! See ADR 0010 for the planned crate rename to `greenfloor-kernel`.

mod fee_budget;
mod inventory;
mod plan;
mod policy;
mod selection;
mod split_planning;

pub use fee_budget::{
    fee_budget_allows_execution, partition_plans_by_budget, projected_coin_ops_fee_mojos,
};
pub use inventory::compute_bucket_counts_from_coins;
pub use plan::{plan_coin_ops, BucketSpec, CoinOpKind, CoinOpPlan};
pub use policy::{
    amount_meets_coin_op_min_mojos, coin_op_min_amount_mojos, coin_op_target_amount_allowed,
};
pub use selection::{
    select_exact_amount_coin_ids, select_largest_spendable_coin,
    select_spendable_coins_for_target_amount, split_would_create_sub_cat_change, SpendableCoin,
};
pub use split_planning::{
    build_combine_prereq_plan, plan_auto_combine_inputs, plan_auto_split_selection,
    CombineInputSelectionMode, SplitAutoSelectPlan, SplitCoinPlan, SplitCombinePrereqPlan,
    SplitPlanningProfile, SplitSkipPlan, SubCatChangeSkipData,
};
