use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::hex::normalize_hex_id;
use crate::vault_coinset_scan::types::AssetTypeFilter;

#[derive(Debug, Clone, Copy)]
pub struct ScanTuningDefaults {
    pub nonce_batch_size: u32,
    pub empty_batch_stop_count: u32,
    pub parent_lookup_batch_size: u32,
}

impl ScanTuningDefaults {
    #[must_use]
    pub const fn vault_cli_defaults() -> Self {
        Self {
            nonce_batch_size: 32,
            empty_batch_stop_count: 1,
            parent_lookup_batch_size: 64,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScanCheckpointControl {
    pub no_resume_checkpoint: bool,
    pub incremental_from_checkpoint: bool,
    pub auto_increment: bool,
}

#[derive(Debug, Clone)]
pub struct ScanRequest {
    pub network: String,
    pub coinset_base_url: Option<String>,
    pub launcher_id: String,
    pub max_nonce: u32,
    pub include_spent: bool,
    pub asset_type: AssetTypeFilter,
    pub requested_cat_ids: HashSet<String>,
    pub requested_cat_tickers: Vec<String>,
    pub checkpoint_file: Option<PathBuf>,
    pub checkpoint_save_interval: u32,
    pub checkpoint: ScanCheckpointControl,
    pub nonce_batch_size: u32,
    pub empty_batch_stop_count: u32,
    pub parent_lookup_batch_size: u32,
    pub start_height: Option<u64>,
    pub end_height: Option<u64>,
    pub cats_config: PathBuf,
    pub markets_config: PathBuf,
    pub testnet_markets_config: Option<PathBuf>,
    pub cache_clear: Option<BTreeMap<String, String>>,
}

/// Shared inputs for manager/engine vault Coinset scans (dust, trace, CLI).
#[derive(Debug, Clone)]
pub struct VaultScanParams<'a> {
    pub network: &'a str,
    pub coinset_base_url: Option<&'a str>,
    pub launcher_id: &'a str,
    pub max_nonce: u32,
    pub include_spent: bool,
    pub asset_type: AssetTypeFilter,
    pub cat_asset_id: Option<&'a str>,
    pub cats_config: &'a Path,
    pub markets_config: &'a Path,
    pub testnet_markets_config: Option<&'a Path>,
}

#[must_use]
pub fn build_vault_scan_request(params: &VaultScanParams<'_>) -> ScanRequest {
    let tuning = ScanTuningDefaults::vault_cli_defaults();
    let requested_cat_ids = params
        .cat_asset_id
        .map(|asset_id| HashSet::from([normalize_hex_id(asset_id)]))
        .unwrap_or_default();
    ScanRequest {
        network: params.network.to_string(),
        coinset_base_url: params
            .coinset_base_url
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        launcher_id: params.launcher_id.to_string(),
        max_nonce: params.max_nonce,
        include_spent: params.include_spent,
        asset_type: params.asset_type,
        requested_cat_ids,
        requested_cat_tickers: Vec::new(),
        checkpoint_file: None,
        checkpoint_save_interval: 1,
        checkpoint: ScanCheckpointControl {
            no_resume_checkpoint: true,
            incremental_from_checkpoint: false,
            auto_increment: false,
        },
        nonce_batch_size: tuning.nonce_batch_size,
        empty_batch_stop_count: tuning.empty_batch_stop_count,
        parent_lookup_batch_size: tuning.parent_lookup_batch_size,
        start_height: None,
        end_height: None,
        cats_config: params.cats_config.to_path_buf(),
        markets_config: params.markets_config.to_path_buf(),
        testnet_markets_config: params.testnet_markets_config.map(Path::to_path_buf),
        cache_clear: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn sample_params(include_spent: bool, asset_type: AssetTypeFilter) -> VaultScanParams<'static> {
        VaultScanParams {
            network: "mainnet",
            coinset_base_url: Some("https://api.coinset.org"),
            launcher_id: "aa",
            max_nonce: 100,
            include_spent,
            asset_type,
            cat_asset_id: Some("bb"),
            cats_config: Path::new("cats.yaml"),
            markets_config: Path::new("markets.yaml"),
            testnet_markets_config: None,
        }
    }

    #[test]
    fn vault_cli_scan_tuning_defaults() {
        let tuning = ScanTuningDefaults::vault_cli_defaults();
        assert_eq!(tuning.nonce_batch_size, 32);
        assert_eq!(tuning.empty_batch_stop_count, 1);
        assert_eq!(tuning.parent_lookup_batch_size, 64);
    }

    #[test]
    fn build_vault_scan_request_sets_include_spent_for_trace() {
        let request = build_vault_scan_request(&sample_params(true, AssetTypeFilter::Cat));
        assert!(request.include_spent);
        assert_eq!(request.asset_type, AssetTypeFilter::Cat);
        assert_eq!(request.requested_cat_ids.len(), 1);
    }

    #[test]
    fn build_vault_scan_request_omits_spent_for_dust() {
        let request = build_vault_scan_request(&sample_params(false, AssetTypeFilter::Cat));
        assert!(!request.include_spent);
    }

    #[test]
    fn build_vault_scan_request_xch_trace_has_empty_cat_filter() {
        let mut params = sample_params(true, AssetTypeFilter::Xch);
        params.cat_asset_id = None;
        let request = build_vault_scan_request(&params);
        assert!(request.requested_cat_ids.is_empty());
        assert_eq!(request.asset_type, AssetTypeFilter::Xch);
    }
}
