use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use crate::vault_coinset_scan::types::AssetTypeFilter;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherIdSource {
    Arg,
    File,
    ProgramConfig,
}

impl LauncherIdSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Arg => "arg",
            Self::File => "file",
            Self::ProgramConfig => "program_config",
        }
    }
}
