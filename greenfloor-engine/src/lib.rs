//! GreenFloor Rust engine: vault KMS signing and deterministic daemon policy.
//!
//! The Rust library crate is named `greenfloor_engine` (ADR 0010). Policy is grouped
//! by domain (`cycle/`, `coin_ops/`, `offer/`, `vault/`).
//!
//! **Import convention:** operator binaries import CLI modules directly
//! (`manager_cli`, `daemon::cli`, `coinset_cli`).

#![recursion_limit = "1024"]
// Watchlist and coin-op selection use implicit `S: Hasher` on HashMap helpers; not worth generic churn.
#![allow(clippy::implicit_hasher)]

pub mod adapters;
pub mod binaries;
pub mod cli_util;
pub mod coin_ops;
pub mod coinset;
pub mod coinset_cli;
pub mod config;
pub mod cycle;
pub mod daemon;
pub mod error;
pub mod hex;
pub mod kms;
pub mod kms_cli;
pub mod manager_cli;
pub mod metrics;
pub mod minimal_program_template;
pub mod offer;
pub mod paths;
pub mod storage;
pub mod vault;

use config::SignerConfig;
use error::SignerResult;

pub use error::SignerError as Error;

pub async fn resolve_vault_context(config: SignerConfig) -> SignerResult<vault::VaultContext> {
    Ok(vault::session::resolve_vault_session(config).await?.display)
}

pub async fn resolve_offer_assets_via_coinset(
    config: SignerConfig,
    base_asset: &str,
    quote_asset: &str,
) -> SignerResult<(String, String)> {
    offer::resolve_offer_assets_via_coinset(&config, base_asset, quote_asset).await
}

/// Deprecated alias for [`resolve_offer_assets_via_coinset`].
pub async fn resolve_offer_asset_ids(
    config: SignerConfig,
    base_asset: &str,
    quote_asset: &str,
) -> SignerResult<(String, String)> {
    resolve_offer_assets_via_coinset(config, base_asset, quote_asset).await
}

#[cfg(test)]
mod test_support;
