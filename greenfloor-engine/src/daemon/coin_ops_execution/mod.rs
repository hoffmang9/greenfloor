mod combine;
mod items;
mod runner;
mod split;

#[cfg(test)]
mod tests;

pub use crate::coin_ops::execution::CoinOpExecContext;
pub use items::{CoinOpExecItem, CoinOpExecutionResult};
pub use runner::{
    execute_managed_coin_op_plans, persist_coin_op_execution, watched_coin_ids_from_open_offers,
};

pub(crate) const COIN_OP_ERROR_PREFIX: &str = "signer_coin_op_error";
