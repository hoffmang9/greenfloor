use serde_json::json;

use crate::cli_util::optional_str;
use crate::coinset::resolve_coinset_endpoint;
use crate::config::{load_program_bundle_gated, operator_ticker_index_from_paths};
use crate::error::SignerResult;
use crate::manager_cli::commands::ManagerCommands;
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::vault_scan::{resolve_manager_vault_launcher, run_manager_vault_scan};
use crate::offer::OfferAssetResolver;
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

pub(crate) fn trace_payload(
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
        resolved_asset.kind.scan_asset_type(),
        resolved_asset
            .kind
            .scan_cat_asset_id(&resolved_asset.asset_id),
    )
    .await?;

    let trace = build_asset_trace(
        &resolved_asset.asset_id,
        resolved_asset.kind.json_label(),
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
    use crate::manager_cli::vault_scan_sim::sim_dust_scan_result;
    use crate::offer::VaultTraceAssetKind;
    use crate::vault_coinset_scan::build_asset_trace;

    #[test]
    fn trace_payload_from_sim_scan_matches_manager_contract() {
        let (scan, _) = sim_dust_scan_result(&[1000, 2000]);
        let asset_id = scan
            .coins
            .first()
            .and_then(|row| row.cat_asset_id.as_deref())
            .expect("cat asset id");
        let trace = build_asset_trace(asset_id, VaultTraceAssetKind::Cat.json_label(), &scan.coins);
        let payload = trace_payload(&scan, &trace, asset_id);

        assert_eq!(payload.get("status"), Some(&json!("ok")));
        assert_eq!(payload.get("network"), Some(&json!("mainnet")));
        assert_eq!(payload.get("launcher_id"), Some(&json!(scan.launcher_id)));
        assert_eq!(payload.get("resolved_asset_id"), Some(&json!(asset_id)));
        assert_eq!(payload.get("asset_type"), Some(&json!("cat")));
        assert_eq!(
            payload.get("lineage_model"),
            Some(&json!("parent_tree_with_same_block_merge_edges"))
        );
        assert_eq!(payload.get("merge_count"), Some(&json!(0)));
        assert_eq!(payload.get("coin_count"), Some(&json!(2)));
        assert_eq!(
            payload
                .get("current_balance")
                .and_then(|value| value.get("unspent_coin_count")),
            Some(&json!(2))
        );
        assert_eq!(
            payload
                .get("scan")
                .and_then(|value| value.get("include_spent")),
            Some(&json!(true))
        );
        assert!(payload.get("coins").and_then(|v| v.as_array()).is_some());
        assert!(payload.get("chains").and_then(|v| v.as_array()).is_some());
        assert!(payload.get("merges").and_then(|v| v.as_array()).is_some());
    }
}
