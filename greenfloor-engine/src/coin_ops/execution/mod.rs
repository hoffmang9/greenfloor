//! Shared coin-op execution context for manager CLI and daemon runner.

mod cap;
mod combine_prereq;
pub mod context;
mod helpers;

#[cfg(debug_assertions)]
mod test_fixtures;

pub use cap::combine_input_coin_cap;
pub use combine_prereq::submit_combine_prereq;
pub use context::CoinOpExecContext;
