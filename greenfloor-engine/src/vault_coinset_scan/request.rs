use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::error::{SignerError, SignerResult};
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

/// When empty nonce batches may end the member walk early.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyBatchStop {
    /// Stop only when the scan has no `start_height` filter.
    WhenUnfiltered,
    /// Always allow empty-batch stop (CAT nonce walks over tall height windows).
    Always,
}

/// How vault Coinset discovery finds member / receive coins.
///
/// Encodes hint queries and optional nonce walks as one plan so callers cannot
/// leave `max_nonce` / hint hashes / empty-batch policy out of sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberDiscovery {
    /// Walk member nonces `0..=max_nonce` (no receive-hint pass).
    Nonces {
        max_nonce: u32,
        empty_batch_stop: EmptyBatchStop,
    },
    /// Discover only via receive / extra puzzle hashes (no member walk).
    Hints { puzzle_hashes: Vec<String> },
    /// Receive/extra hints first, then walk member nonces `0..=max_nonce`.
    HintsThenNonces {
        puzzle_hashes: Vec<String>,
        max_nonce: u32,
        empty_batch_stop: EmptyBatchStop,
    },
}

impl MemberDiscovery {
    /// Standard nonce walk with empty-batch stop only when unfiltered by height.
    #[must_use]
    pub fn nonces(max_nonce: u32) -> Self {
        Self::Nonces {
            max_nonce,
            empty_batch_stop: EmptyBatchStop::WhenUnfiltered,
        }
    }

    #[must_use]
    pub fn hint_puzzle_hashes(&self) -> &[String] {
        match self {
            Self::Nonces { .. } => &[],
            Self::Hints { puzzle_hashes } | Self::HintsThenNonces { puzzle_hashes, .. } => {
                puzzle_hashes.as_slice()
            }
        }
    }

    #[must_use]
    pub fn max_nonce(&self) -> Option<u32> {
        match self {
            Self::Hints { .. } => None,
            Self::Nonces { max_nonce, .. } | Self::HintsThenNonces { max_nonce, .. } => {
                Some(*max_nonce)
            }
        }
    }

    #[must_use]
    pub fn empty_batch_stop(&self) -> EmptyBatchStop {
        match self {
            Self::Hints { .. } => EmptyBatchStop::WhenUnfiltered,
            Self::Nonces {
                empty_batch_stop, ..
            }
            | Self::HintsThenNonces {
                empty_batch_stop, ..
            } => *empty_batch_stop,
        }
    }

    /// Discovery plan for `vault-asset-trace`: XCH walks member nonces; CAT uses receive hints
    /// and optionally a nonce walk when `--max-nonce` is set.
    ///
    /// # Errors
    ///
    /// Returns an error when CAT tracing has neither receive hints nor `--max-nonce`.
    pub fn for_vault_asset_trace(
        asset_type: AssetTypeFilter,
        max_nonce: Option<u32>,
        cat_hint_puzzle_hashes: Vec<String>,
    ) -> SignerResult<Self> {
        match asset_type {
            AssetTypeFilter::Xch | AssetTypeFilter::All => {
                Ok(Self::nonces(max_nonce.unwrap_or(100)))
            }
            AssetTypeFilter::Cat => match max_nonce {
                None => {
                    if cat_hint_puzzle_hashes.is_empty() {
                        return Err(SignerError::Other(
                            "vault-asset-trace CAT path needs a market receive_address for the asset, \
                             or pass --max-nonce N to scan vault member nonces"
                                .to_string(),
                        ));
                    }
                    Ok(Self::Hints {
                        puzzle_hashes: cat_hint_puzzle_hashes,
                    })
                }
                Some(max_nonce) => Ok(Self::HintsThenNonces {
                    puzzle_hashes: cat_hint_puzzle_hashes,
                    max_nonce,
                    empty_batch_stop: EmptyBatchStop::Always,
                }),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScanRequest {
    pub network: String,
    pub coinset_base_url: Option<String>,
    pub launcher_id: String,
    pub discovery: MemberDiscovery,
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
    pub discovery: MemberDiscovery,
    pub start_height: Option<u64>,
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
        discovery: params.discovery.clone(),
        start_height: params.start_height,
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
            discovery: MemberDiscovery::nonces(100),
            start_height: None,
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
        let mut params = sample_params(true, AssetTypeFilter::Cat);
        params.start_height = Some(8_376_742);
        let request = build_vault_scan_request(&params);
        assert!(request.include_spent);
        assert_eq!(request.asset_type, AssetTypeFilter::Cat);
        assert_eq!(request.requested_cat_ids.len(), 1);
        assert_eq!(request.start_height, Some(8_376_742));
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

    #[test]
    fn member_discovery_hints_skip_nonce_walk() {
        let hints = MemberDiscovery::Hints {
            puzzle_hashes: vec!["aa".repeat(32)],
        };
        assert!(hints.max_nonce().is_none());
        assert_eq!(hints.hint_puzzle_hashes().len(), 1);

        let nonces = MemberDiscovery::nonces(0);
        assert_eq!(nonces.max_nonce(), Some(0));
        assert!(nonces.hint_puzzle_hashes().is_empty());
    }

    #[test]
    fn member_discovery_hints_then_nonces_always_stops_empty_batches() {
        let plan = MemberDiscovery::HintsThenNonces {
            puzzle_hashes: vec!["aa".repeat(32)],
            max_nonce: 3,
            empty_batch_stop: EmptyBatchStop::Always,
        };
        assert_eq!(plan.max_nonce(), Some(3));
        assert_eq!(plan.empty_batch_stop(), EmptyBatchStop::Always);
        assert_eq!(plan.hint_puzzle_hashes().len(), 1);
    }

    #[test]
    fn xch_discovery_is_nonce_walk_without_hints() {
        let plan = MemberDiscovery::for_vault_asset_trace(
            AssetTypeFilter::Xch,
            None,
            vec!["should-be-ignored".to_string()],
        )
        .expect("xch plan");
        assert!(matches!(
            plan,
            MemberDiscovery::Nonces { max_nonce: 100, .. }
        ));
        assert!(plan.hint_puzzle_hashes().is_empty());
    }

    #[test]
    fn cat_discovery_defaults_to_hints_only() {
        let hashes = vec!["aa".repeat(32)];
        let plan =
            MemberDiscovery::for_vault_asset_trace(AssetTypeFilter::Cat, None, hashes.clone())
                .expect("cat hints");
        assert_eq!(
            plan,
            MemberDiscovery::Hints {
                puzzle_hashes: hashes
            }
        );
    }

    #[test]
    fn cat_discovery_without_hints_or_nonce_errors() {
        let err = MemberDiscovery::for_vault_asset_trace(AssetTypeFilter::Cat, None, Vec::new())
            .expect_err("needs hints or max-nonce");
        assert!(err.to_string().contains("receive_address"));
    }

    #[test]
    fn cat_discovery_with_max_nonce_uses_hints_then_nonces() {
        let hashes = vec!["aa".repeat(32)];
        let plan =
            MemberDiscovery::for_vault_asset_trace(AssetTypeFilter::Cat, Some(7), hashes.clone())
                .expect("cat plan");
        assert_eq!(
            plan,
            MemberDiscovery::HintsThenNonces {
                puzzle_hashes: hashes,
                max_nonce: 7,
                empty_batch_stop: EmptyBatchStop::Always,
            }
        );
    }
}
