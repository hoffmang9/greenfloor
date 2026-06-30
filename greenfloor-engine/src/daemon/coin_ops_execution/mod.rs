mod combine;
mod items;
mod prep;
mod runner;
mod split;

#[cfg(test)]
mod tests;

pub use items::{CoinOpExecItem, CoinOpExecutionResult};
#[cfg(test)]
pub use runner::execute_managed_coin_op_plans_with_test_overrides;
pub use runner::{
    execute_managed_coin_op_plans, persist_coin_op_execution, watched_coin_ids_from_open_offers,
};

pub(crate) const COIN_OP_ERROR_PREFIX: &str = "signer_coin_op_error";
