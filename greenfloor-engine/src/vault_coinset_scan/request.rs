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
    pub no_resume_checkpoint: bool,
    pub nonce_batch_size: u32,
    pub empty_batch_stop_count: u32,
    pub parent_lookup_batch_size: u32,
    pub start_height: Option<u64>,
    pub end_height: Option<u64>,
    pub incremental_from_checkpoint: bool,
    pub auto_increment: bool,
    pub cats_config: PathBuf,
    pub markets_config: PathBuf,
    pub testnet_markets_config: Option<PathBuf>,
    pub cache_clear: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone)]
pub struct CatDustScanParams<'a> {
    pub network: &'a str,
    pub coinset_base_url: Option<&'a str>,
    pub launcher_id: &'a str,
    pub max_nonce: u32,
    pub cat_asset_id: &'a str,
    pub cats_config: &'a Path,
    pub markets_config: &'a Path,
    pub testnet_markets_config: Option<&'a Path>,
}

#[must_use]
pub fn build_cat_dust_scan_request(params: &CatDustScanParams<'_>) -> ScanRequest {
    let tuning = ScanTuningDefaults::vault_cli_defaults();
    let cat_asset_id = normalize_hex_id(params.cat_asset_id);
    ScanRequest {
        network: params.network.to_string(),
        coinset_base_url: params
            .coinset_base_url
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        launcher_id: params.launcher_id.to_string(),
        max_nonce: params.max_nonce,
        include_spent: false,
        asset_type: AssetTypeFilter::Cat,
        requested_cat_ids: HashSet::from([cat_asset_id]),
        requested_cat_tickers: Vec::new(),
        checkpoint_file: None,
        checkpoint_save_interval: 1,
        no_resume_checkpoint: true,
        nonce_batch_size: tuning.nonce_batch_size,
        empty_batch_stop_count: tuning.empty_batch_stop_count,
        parent_lookup_batch_size: tuning.parent_lookup_batch_size,
        start_height: None,
        end_height: None,
        incremental_from_checkpoint: false,
        auto_increment: false,
        cats_config: params.cats_config.to_path_buf(),
        markets_config: params.markets_config.to_path_buf(),
        testnet_markets_config: params.testnet_markets_config.map(Path::to_path_buf),
        cache_clear: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_cli_scan_tuning_defaults() {
        let tuning = ScanTuningDefaults::vault_cli_defaults();
        assert_eq!(tuning.nonce_batch_size, 32);
        assert_eq!(tuning.empty_batch_stop_count, 1);
        assert_eq!(tuning.parent_lookup_batch_size, 64);
    }
}
