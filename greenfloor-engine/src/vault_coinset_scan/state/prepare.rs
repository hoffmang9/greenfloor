use std::collections::{BTreeMap, HashMap, HashSet};

use chia_protocol::Bytes32;

use crate::coinset::{resolve_direct_client, DirectCoinsetScanClient};
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::vault::members::hex_to_bytes32;
use crate::vault_coinset_scan::checkpoint::{load_scan_checkpoint, LoadedCheckpoint};
use crate::vault_coinset_scan::metadata::{load_scan_cat_ticker_index, resolve_requested_cat_ids};
use crate::vault_coinset_scan::request::ScanRequest;
use crate::vault_coinset_scan::types::AssetTypeFilter;
use crate::vault_coinset_scan::window::{resolve_scan_window, ScanWindowPlan};

pub(super) struct ScanMetadata {
    pub effective_asset_type: AssetTypeFilter,
    pub requested_cat_ids: HashSet<String>,
    pub asset_id_to_symbols: BTreeMap<String, Vec<String>>,
}

pub(super) struct ResolvedScanClient {
    pub scanner: DirectCoinsetScanClient,
    pub launcher_id: String,
    pub launcher_bytes: Bytes32,
}

pub(super) fn resolve_scan_client(request: &ScanRequest) -> SignerResult<ResolvedScanClient> {
    let resolved = resolve_direct_client(&request.network, request.coinset_base_url.as_deref());
    let scanner = DirectCoinsetScanClient::new(resolved.network, Some(resolved.base_url.as_str()));

    let launcher_id = normalize_hex_id(&request.launcher_id);
    if launcher_id.is_empty() {
        return Err(SignerError::Other("launcher id is required".to_string()));
    }
    let launcher_bytes = hex_to_bytes32(&launcher_id)?;

    Ok(ResolvedScanClient {
        scanner,
        launcher_id,
        launcher_bytes,
    })
}

pub(super) fn resolve_scan_metadata(request: &ScanRequest) -> SignerResult<ScanMetadata> {
    let (ticker_to_asset_ids, asset_id_to_symbols) = load_scan_cat_ticker_index(
        &request.cats_config,
        &request.markets_config,
        request.testnet_markets_config.as_deref(),
    );

    let requested_cat_ids_raw = request
        .requested_cat_ids
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let (requested_cat_ids, unresolved_cat_tickers) = resolve_requested_cat_ids(
        &requested_cat_ids_raw,
        &request.requested_cat_tickers,
        &ticker_to_asset_ids,
    );
    if !unresolved_cat_tickers.is_empty() {
        return Err(SignerError::Other(format!(
            "unknown cat ticker(s): {}",
            unresolved_cat_tickers.join(", ")
        )));
    }

    let effective_asset_type =
        if !requested_cat_ids.is_empty() || !request.requested_cat_tickers.is_empty() {
            AssetTypeFilter::Cat
        } else {
            request.asset_type
        };

    Ok(ScanMetadata {
        effective_asset_type,
        requested_cat_ids,
        asset_id_to_symbols,
    })
}

pub(super) fn load_checkpoint_or_default(
    request: &ScanRequest,
    network: &str,
    launcher_id: &str,
) -> SignerResult<LoadedCheckpoint> {
    let checkpoint_enabled = request.checkpoint_file.is_some();
    if checkpoint_enabled && !request.no_resume_checkpoint {
        let checkpoint_file = request.checkpoint_file.as_ref().expect("checkpoint file");
        load_scan_checkpoint(checkpoint_file, network, launcher_id, request.include_spent)
    } else {
        Ok(LoadedCheckpoint {
            start_nonce: 0,
            nonce_to_p2: HashMap::new(),
            by_coin_id: HashMap::new(),
            cat_asset_cache: HashMap::new(),
            parent_lineage_cache: HashMap::new(),
            last_synced_height: None,
            discarded_mismatch: false,
        })
    }
}

pub(super) async fn resolve_effective_window(
    scanner: &DirectCoinsetScanClient,
    request: &ScanRequest,
    checkpoint_enabled: bool,
    checkpoint_last_synced_height: Option<u64>,
) -> SignerResult<ScanWindowPlan> {
    if request.incremental_from_checkpoint && !checkpoint_enabled {
        return Err(SignerError::Other(
            "--incremental-from-checkpoint requires --checkpoint-file".to_string(),
        ));
    }

    let chain_peak_height = if request.incremental_from_checkpoint || request.end_height.is_none() {
        scanner.chain_peak_height().await?
    } else {
        None
    };

    resolve_scan_window(
        request.start_height,
        request.end_height,
        request.incremental_from_checkpoint,
        checkpoint_last_synced_height,
        chain_peak_height,
    )
}
