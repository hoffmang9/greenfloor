mod combine;
mod context;
pub mod helpers;
mod items;
mod runner;
mod split;

pub use context::CoinOpExecContext;
pub use items::{CoinOpExecItem, CoinOpExecutionResult};
pub use runner::{
    combine_input_coin_cap, execute_managed_coin_op_plans, persist_coin_op_execution,
    watched_coin_ids_for_market,
};

pub(crate) const COIN_OP_ERROR_PREFIX: &str = "signer_coin_op_error";
