//! Deterministic coin-operation policy (planning, fee budget, inventory buckets).
//!
//! Lives in the `greenfloor-engine` crate alongside vault signing and cycle policy.

mod amounts;
mod effective_counts;
pub mod execution;
mod fee_budget;
mod gate;
mod inventory;
mod plan;
mod policy;
mod scalars;
mod selection;
mod split_planning;
mod wallet_coin;

pub use amounts::{combine_output_amounts, total_for_coin_ids};
pub use effective_counts::effective_sell_bucket_counts_for_coin_ops;
pub use execution::CoinOpExecContext;
pub use fee_budget::{
    fee_budget_allows_execution, partition_plans_by_budget, projected_coin_ops_fee_mojos,
};
pub use gate::{
    coin_op_should_stop, evaluate_coin_combine_gate, evaluate_coin_split_gate,
    CoinCombineGateResult, CoinSplitGateResult,
};
pub use inventory::compute_bucket_counts_from_coins;
pub use plan::{
    plan_coin_ops, BucketSpec, CoinOpKind, CoinOpPlan, CoinOpPlanningResult, LadderTargetRow,
};
pub use policy::{
    amount_meets_coin_op_min_mojos, coin_op_min_amount_mojos, coin_op_target_amount_allowed,
};
pub use scalars::{
    coin_op_non_negative_u64, coin_op_non_negative_u64_saturating, i64_to_usize, usize_to_i64,
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
pub use wallet_coin::{is_spendable_coin_state, is_spendable_wallet_coin};
