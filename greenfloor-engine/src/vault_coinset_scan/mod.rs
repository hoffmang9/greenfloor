//! Vault Coinset scan: nonce-based member puzzle hash discovery and CAT classification.

pub mod cat_detect;
pub mod checkpoint;
pub mod cli;
pub mod dust;
pub mod launcher;
pub mod metadata;
pub mod request;
pub mod result;
pub mod state;
pub mod types;
pub mod window;

pub use cli::{run_vault_coinset_scan_command, VaultCoinsetScanCliArgs};
pub use dust::{
    dust_coins_from_scan, plan_dust_batches, plan_dust_from_scan_with_lineage,
    prove_dust_coins_lineage, DustBatchPlan, DustCoin, DustCombineBatch, DustPlan,
};
pub use launcher::{
    cache_resolved_launcher_id, resolve_launcher_id, LauncherIdSource, ResolveLauncherIdParams,
    ResolvedLauncherId,
};
pub use request::{
    build_cat_dust_scan_request, CatDustScanParams, ScanRequest, ScanTuningDefaults,
};
pub use result::ScanResult;
pub use state::ScanState;
