//! Vault Coinset scan: nonce-based member puzzle hash discovery and CAT classification.

pub mod asset_trace;
pub mod cat_detect;
pub mod checkpoint;
pub mod cli;
pub mod dust;
#[cfg(test)]
mod dust_lineage_test;
pub mod launcher;
pub mod metadata;
pub mod request;
pub mod result;
pub mod state;
pub mod types;
pub mod window;

pub use asset_trace::build_asset_trace;
pub use cli::{run_vault_coinset_scan_command, VaultCoinsetScanCliArgs};
pub use dust::{
    dust_coins_from_scan, plan_dust_batches, plan_dust_from_scan_with_lineage,
    prove_dust_coins_lineage, DustBatchPlan, DustCoin, DustCombineBatch, DustPlan, ProvenDustCoin,
};
pub use launcher::{
    cache_resolved_launcher_id, resolve_launcher_id, LauncherIdSource, ResolveLauncherIdParams,
    ResolvedLauncherId,
};
pub use request::{build_vault_scan_request, ScanRequest, ScanTuningDefaults, VaultScanParams};
pub use result::ScanResult;
pub use state::ScanState;
