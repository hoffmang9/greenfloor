use serde_json::json;

use crate::cli_util::optional_str;
use crate::coinset::resolve_coinset_endpoint;
use crate::config::{load_program_bundle_gated, operator_ticker_index_from_paths};
use crate::error::SignerResult;
use crate::manager_cli::commands::ManagerCommands;
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::vault_scan::{resolve_manager_vault_launcher, run_manager_vault_scan};
use crate::offer::{OfferAssetResolver, VaultTraceAssetKind};
use crate::vault_coinset_scan::types::AssetTypeFilter;
use crate::vault_coinset_scan::{build_asset_trace, ScanResult};

pub struct VaultAssetTraceRequest<'a> {
    pub mgr: &'a ManagerContext,
    pub network: Option<&'a str>,
    pub coinset_base_url: Option<&'a str>,
    pub launcher_id: Option<&'a str>,
    pub launcher_id_file: Option<&'a str>,
    pub max_nonce: u32,
    pub asset: &'a str,
}

fn asset_type_label(kind: VaultTraceAssetKind) -> &'static str {
    match kind {
        VaultTraceAssetKind::Xch => "xch",
        VaultTraceAssetKind::Cat => "cat",
    }
}

fn asset_type_filter(kind: VaultTraceAssetKind) -> AssetTypeFilter {
    match kind {
        VaultTraceAssetKind::Xch => AssetTypeFilter::Xch,
        VaultTraceAssetKind::Cat => AssetTypeFilter::Cat,
    }
}

fn trace_payload(
    scan: &ScanResult,
    trace: &crate::vault_coinset_scan::asset_trace::AssetTraceResult,
    requested_asset: &str,
) -> serde_json::Value {
    json!({
        "status": "ok",
        "network": scan.network,
        "launcher_id": scan.launcher_id,
        "requested_asset": requested_asset.trim(),
        "resolved_asset_id": trace.asset_id,
        "asset_type": trace.asset_type,
        "lineage_model": trace.lineage_model,
        "scan": {
            "coin_count": scan.count,
            "max_nonce_scanned": scan.max_nonce_scanned,
            "scan_stop_reason": scan.scan_stop_reason,
            "include_spent": true,
        },
        "current_balance": trace.current_balance,
        "reception_count": trace.reception_count,
        "merge_count": trace.merge_count,
        "coin_count": trace.coin_count,
        "coins": trace.coins,
        "chains": trace.chains,
        "merges": trace.merges,
    })
}

pub async fn run_vault_asset_trace(request: VaultAssetTraceRequest<'_>) -> SignerResult<i32> {
    let mgr = request.mgr;
    let bundle = load_program_bundle_gated(&mgr.program_config)?;
    let network = request
        .network
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(bundle.program.network.as_str());
    let coinset = resolve_coinset_endpoint(
        network,
        &bundle.signer.coinset_base_url,
        request.coinset_base_url,
    );
    let ticker_index = operator_ticker_index_from_paths(
        &mgr.markets_config,
        mgr.testnet_markets_path(),
        Some(&mgr.cats_config),
    );
    let resolver = OfferAssetResolver::new(&bundle.signer, &ticker_index, network);
    let resolved_asset = resolver.resolve_vault_trace_asset(request.asset).await?;
    let launcher =
        resolve_manager_vault_launcher(mgr, request.launcher_id, request.launcher_id_file)?;

    let scan = run_manager_vault_scan(
        mgr,
        &coinset,
        &launcher.launcher_id,
        request.max_nonce,
        true,
        asset_type_filter(resolved_asset.kind),
        match resolved_asset.kind {
            VaultTraceAssetKind::Cat => Some(resolved_asset.asset_id.as_str()),
            VaultTraceAssetKind::Xch => None,
        },
    )
    .await?;

    let trace = build_asset_trace(
        &resolved_asset.asset_id,
        asset_type_label(resolved_asset.kind),
        &scan.coins,
    );

    mgr.emit_json(&trace_payload(&scan, &trace, request.asset))?;
    Ok(0)
}

pub async fn run_vault_asset_trace_command(
    command: ManagerCommands,
    ctx: &ManagerContext,
) -> SignerResult<i32> {
    let ManagerCommands::VaultAssetTrace {
        network,
        coinset_base_url,
        launcher_id,
        launcher_id_file,
        max_nonce,
        asset,
    } = command
    else {
        unreachable!("run_vault_asset_trace_command called with {command:?}");
    };

    Box::pin(run_vault_asset_trace(VaultAssetTraceRequest {
        mgr: ctx,
        network: optional_str(&network),
        coinset_base_url: optional_str(&coinset_base_url),
        launcher_id: optional_str(&launcher_id),
        launcher_id_file: optional_str(&launcher_id_file),
        max_nonce,
        asset: asset.as_str(),
    }))
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{load_program_bundle_gated, operator_ticker_index_from_paths};
    use crate::offer::OfferAssetResolver;

    #[tokio::test]
    async fn resolve_vault_trace_asset_accepts_xch_aliases() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("program.yaml");
        crate::test_support::minimal_program::write_minimal_program_with_signer(
            &program,
            crate::test_support::minimal_program::MinimalProgramParams::default(),
        );
        let bundle = load_program_bundle_gated(&program).expect("bundle");
        let index =
            operator_ticker_index_from_paths(&dir.path().join("missing-markets.yaml"), None, None);
        let resolver = OfferAssetResolver::new(&bundle.signer, &index, "mainnet");
        let resolved_asset = resolver
            .resolve_vault_trace_asset("txch")
            .await
            .expect("xch");
        assert_eq!(resolved_asset.kind, VaultTraceAssetKind::Xch);
        assert_eq!(resolved_asset.asset_id, "xch");
    }
}
