//! Shared coin-op execution context for manager CLI and daemon runner.

mod cap;
mod combine_prereq;
pub mod context;
mod helpers;
mod test_overrides;

pub use cap::resolve_combine_input_cap;
pub use combine_prereq::submit_combine_prereq;
pub use context::CoinOpExecContext;
pub use test_overrides::CoinOpTestOverrides;
