//! Typed async boundaries for operator entrypoints.
//!
//! Box once at module entrypoints that return `impl Future` across crate boundaries.
//! Internal call chains use plain `async fn` (no nested `Box::pin`).
//!
//! ## `Box::pin` inventory (Clippy `large_futures` only)
//!
//! These are the only intentional heap boxes. Do not add nested pins inside call chains.
//!
//! | Site | Role |
//! |------|------|
//! | `manager_cli/commands/run/mod.rs` | `ManagerCommands::run` → boxed command future |
//! | `manager_cli/commands/run/dust.rs` | combine-market-cat-dust command arm |
//! | `manager_cli/coin_op_loop/split.rs` | `run_coin_split` entry |
//! | `manager_cli/coin_op_loop/combine.rs` | `run_coin_combine` entry |
//! | `manager_cli/combine_market_cat_dust/mod.rs` | per-job scan/finalize arms |
//! | `offer/operator/build_and_post/mod.rs` | managed offer build/post |
//! | `offer/build.rs` | vault CAT offer construction |
//! | `offer/operator/signer_denomination/mod.rs` | denomination bootstrap phase |
//! | `daemon/offer_dispatch/mod.rs` | strategy action dispatch |
//! | `daemon/offer_dispatch/managed_post.rs` | single managed offer post |
//! | `daemon/market_cycle.rs` | post-reconcile market phases |
//! | `daemon/coin_ops_execution/runner.rs` | managed coin-op plan execution |
//! | `main.rs` | daemon / create-offer CLI arms only |
//!
//! `combine_market_cat_dust/execute.rs` uses a `BatchDriver` trait instead of callback pins.

mod daemon;
mod manager;
mod offer;

pub use daemon::{ManagedCoinOpPlansFuture, ManagedOfferPostFuture, StrategyDispatchFuture};
pub use manager::ManagerCommandFuture;
pub use offer::{BuildAndPostOfferFuture, BuildVaultCatOfferFuture};
