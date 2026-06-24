//! Shared coin-op execution context for manager CLI and daemon runner.

mod cap;
pub mod context;
mod helpers;
#[cfg(test)]
mod test_overrides;

pub use cap::resolve_combine_input_cap;
pub use context::CoinOpExecContext;
#[cfg(test)]
pub use test_overrides::CoinOpTestOverrides;
