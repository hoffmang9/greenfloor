//! Coin split/combine CLI iteration loops using canonical gate policy.

mod combine;
mod context;
mod list;
mod loop_common;
mod split;

#[cfg(test)]
mod tests;

pub use combine::run_coin_combine;
pub use list::{run_coin_status, run_coins_list};
pub use split::run_coin_split;
