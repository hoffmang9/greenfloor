//! Shared market-gated coin-op execution for manager CLI and daemon.
//!
//! Offer bootstrap and dust combine call the vault submitter directly
//! (`build_and_optionally_broadcast_vault_cat_mixed_split` /
//! `submit_vault_cat_mixed_split` + `CatSelection`).

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
