//! Typed async boundaries for operator entrypoints.
//!
//! Box once at module entrypoints that return `impl Future` across crate boundaries.
//! Internal call chains use plain `async fn` (no nested `Box::pin`).

mod daemon;
mod manager;
mod offer;

pub use daemon::{ManagedCoinOpPlansFuture, ManagedOfferPostFuture, StrategyDispatchFuture};
pub use manager::ManagerCommandFuture;
pub use offer::{BuildAndPostOfferFuture, BuildVaultCatOfferFuture};
