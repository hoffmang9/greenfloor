use std::collections::HashMap;

use crate::vault_coinset_scan::types::CoinRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadCheckpointDiscardReason {
    LauncherIdMismatch,
    NetworkMismatch,
    IncludeSpentMismatch,
}

impl LoadCheckpointDiscardReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LauncherIdMismatch => "launcher_id_mismatch",
            Self::NetworkMismatch => "network_mismatch",
            Self::IncludeSpentMismatch => "include_spent_mismatch",
        }
    }
}

#[derive(Debug)]
pub enum LoadCheckpointResult {
    Loaded {
        checkpoint: Box<LoadedCheckpoint>,
        start_nonce: u32,
    },
    Discarded(LoadCheckpointDiscardReason),
}

impl LoadCheckpointResult {
    #[must_use]
    pub fn empty() -> Self {
        Self::Loaded {
            checkpoint: Box::new(LoadedCheckpoint::empty()),
            start_nonce: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParentLineageEntry {
    pub spent_height: u64,
    pub child_asset_ids: HashMap<String, String>,
}

/// Resume payload persisted in checkpoint files and held live during scans.
#[derive(Debug, Clone)]
pub struct LoadedCheckpoint {
    pub nonce_to_p2: HashMap<u32, String>,
    pub by_coin_id: HashMap<String, CoinRow>,
    pub cat_asset_cache: HashMap<String, String>,
    pub parent_lineage_cache: HashMap<String, ParentLineageEntry>,
    pub last_synced_height: Option<u64>,
}

impl LoadedCheckpoint {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            nonce_to_p2: HashMap::new(),
            by_coin_id: HashMap::new(),
            cat_asset_cache: HashMap::new(),
            parent_lineage_cache: HashMap::new(),
            last_synced_height: None,
        }
    }

    #[must_use]
    pub fn max_nonce_scanned(&self) -> u32 {
        self.nonce_to_p2.keys().copied().max().unwrap_or(0)
    }
}
