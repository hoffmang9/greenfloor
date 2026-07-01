use crate::coinset::ResolvedCoinsetEndpoint;
use crate::error::SignerResult;
use crate::manager_cli::context::ManagerContext;
use crate::vault_coinset_scan::types::AssetTypeFilter;
use crate::vault_coinset_scan::{
    build_vault_scan_request, cache_resolved_launcher_id, resolve_launcher_id,
    ResolveLauncherIdParams, ResolvedLauncherId, ScanResult, ScanState, VaultScanParams,
};

/// Resolve vault launcher id from CLI overrides and cache when configured.
///
/// # Errors
///
/// Returns an error if launcher resolution or cache write fails.
pub fn resolve_manager_vault_launcher(
    mgr: &ManagerContext,
    launcher_id: Option<&str>,
    launcher_id_file: Option<&str>,
) -> SignerResult<ResolvedLauncherId> {
    let resolved = resolve_launcher_id(&ResolveLauncherIdParams {
        launcher_id,
        launcher_id_file,
        program_config: Some(mgr.program_config.as_path()),
        preloaded_program: None,
    })?;
    cache_resolved_launcher_id(launcher_id_file, resolved.source, &resolved.launcher_id)?;
    Ok(resolved)
}

/// Build vault scan parameters from manager paths and a resolved Coinset endpoint.
#[must_use]
pub fn manager_vault_scan_params<'a>(
    mgr: &'a ManagerContext,
    coinset: &'a ResolvedCoinsetEndpoint,
    launcher_id: &'a str,
    max_nonce: u32,
    include_spent: bool,
    asset_type: AssetTypeFilter,
    cat_asset_id: Option<&'a str>,
) -> VaultScanParams<'a> {
    VaultScanParams {
        network: coinset.network,
        coinset_base_url: Some(coinset.base_url()),
        launcher_id,
        max_nonce,
        include_spent,
        asset_type,
        cat_asset_id,
        cats_config: &mgr.cats_config,
        markets_config: &mgr.markets_config,
        testnet_markets_config: mgr.testnet_markets_path(),
    }
}

/// Run a vault Coinset scan using shared manager config paths.
///
/// # Errors
///
/// Returns an error if the scan fails.
pub async fn run_manager_vault_scan(params: VaultScanParams<'_>) -> SignerResult<ScanResult> {
    ScanState::run(build_vault_scan_request(&params)).await
}
