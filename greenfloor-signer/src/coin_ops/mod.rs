//! Deterministic coin-operation policy (planning, fee budget, inventory buckets).
//!
//! Lives in the `greenfloor-signer` crate alongside vault signing and cycle policy.
//! See ADR 0010 for the planned crate rename to `greenfloor-kernel`.

mod fee_budget;
mod inventory;
mod plan;
mod policy;

pub use fee_budget::{
    fee_budget_allows_execution, partition_plans_by_budget, projected_coin_ops_fee_mojos,
};
pub use inventory::compute_bucket_counts_from_coins;
pub use plan::{plan_coin_ops, BucketSpec, CoinOpKind, CoinOpPlan};
pub use policy::{
    amount_meets_coin_op_min_mojos, coin_op_min_amount_mojos, coin_op_target_amount_allowed,
};
