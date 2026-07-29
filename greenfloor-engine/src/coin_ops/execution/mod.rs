//! Shared coin-op execution for manager CLI, daemon, and offer bootstrap.
//!
//! Dust combine submits via `vault::submit_vault_cat_mixed_split` with
//! `CatSelection::Preselected` (outside this market-gated context).

mod cap;
pub mod context;
mod helpers;
pub mod managed;
#[cfg(test)]
mod test_overrides;

pub use cap::resolve_combine_input_cap;
pub use context::CoinOpExecContext;
#[cfg(test)]
pub use managed::execute_managed_coin_op_plans_with_test_overrides;
pub use managed::{
    execute_managed_coin_op_plans, persist_coin_op_execution, CoinOpExecItem, CoinOpExecutionResult,
};
#[cfg(test)]
pub use test_overrides::CoinOpTestOverrides;
