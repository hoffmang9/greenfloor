//! Shared test helpers (submodules are `#[cfg(test)]` only).
#![allow(clippy::large_futures)] // simulator offer paths exceed Clippy threshold; not production code.

#[cfg(test)]
pub mod build_and_post;
#[cfg(test)]
pub mod eco181_inventory;
#[cfg(test)]
mod export_fixtures_test;
#[cfg(test)]
pub mod golden;
#[cfg(test)]
pub mod injections;
#[cfg(test)]
pub mod ladder;
#[cfg(test)]
pub mod market_config;
#[cfg(test)]
pub mod minimal_program;
#[cfg(test)]
pub mod noop_coinset;
#[cfg(test)]
pub mod signer_config;
#[cfg(test)]
pub mod simulator;
