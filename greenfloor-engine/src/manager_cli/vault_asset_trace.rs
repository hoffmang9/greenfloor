use serde::Serialize;

use crate::cli_util::optional_str;
use crate::coinset::{puzzle_hash_hex_for_receive_address, resolve_coinset_endpoint};
use crate::config::{
    load_markets_config_with_overlay, load_program_bundle_gated, operator_ticker_index_from_paths,
    CatTickerIndex, MarketConfig,
};
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::manager_cli::commands::ManagerCommands;
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::vault_scan::{
    manager_vault_scan_params, resolve_manager_vault_launcher, run_manager_vault_scan,
};
use crate::offer::OfferAssetResolver;
use crate::offer::VaultTraceAssetKind;
use crate::vault_coinset_scan::asset_trace::AssetTraceResult;
use crate::vault_coinset_scan::types::AssetTypeFilter;
use crate::vault_coinset_scan::{build_asset_trace, EmptyBatchStop, MemberDiscovery, ScanResult};

impl VaultTraceAssetKind {
    #[must_use]
    fn json_label(self) -> &'static str {
        match self {
            Self::Xch => "xch",
            Self::Cat => "cat",
        }
    }

    #[must_use]
    fn scan_asset_type(self) -> AssetTypeFilter {
        match self {
            Self::Xch => AssetTypeFilter::Xch,
            Self::Cat => AssetTypeFilter::Cat,
        }
    }

    #[must_use]
    fn scan_cat_asset_id(self, asset_id: &str) -> Option<&str> {
        match self {
            Self::Cat => Some(asset_id),
            Self::Xch => None,
        }
    }
}

pub struct VaultAssetTraceRequest<'a> {
    pub mgr: &'a ManagerContext,
    pub network: Option<&'a str>,
    pub coinset_base_url: Option<&'a str>,
    pub launcher_id: Option<&'a str>,
    pub launcher_id_file: Option<&'a str>,
    pub max_nonce: Option<u32>,
    pub start_height: Option<u64>,
    pub asset: &'a str,
}

#[derive(Serialize)]
struct VaultAssetTraceScanMeta {
    scanned_row_count: usize,
    max_nonce_scanned: u32,
    start_height: Option<u64>,
    scan_stop_reason: crate::vault_coinset_scan::types::ScanStopReason,
    include_spent: bool,
}

#[derive(Serialize)]
struct VaultAssetTracePayload<'a> {
    status: &'static str,
    network: String,
    launcher_id: String,
    requested_asset: String,
    #[serde(flatten)]
    lineage: &'a AssetTraceResult,
    scan: VaultAssetTraceScanMeta,
}

impl<'a> VaultAssetTracePayload<'a> {
    fn new(scan: &'a ScanResult, trace: &'a AssetTraceResult, requested_asset: &str) -> Self {
        Self {
            status: "ok",
            network: scan.network.clone(),
            launcher_id: scan.launcher_id.clone(),
            requested_asset: requested_asset.trim().to_string(),
            lineage: trace,
            scan: VaultAssetTraceScanMeta {
                scanned_row_count: scan.count,
                max_nonce_scanned: scan.max_nonce_scanned,
                start_height: scan.scan_window.start_height,
                scan_stop_reason: scan.scan_stop_reason,
                include_spent: true,
            },
        }
    }
}

pub(crate) fn trace_payload(
    scan: &ScanResult,
    trace: &AssetTraceResult,
    requested_asset: &str,
) -> SignerResult<serde_json::Value> {
    serde_json::to_value(VaultAssetTracePayload::new(scan, trace, requested_asset))
        .map_err(|err| SignerError::Other(err.to_string()))
}

/// Build the vault-asset-trace discovery plan.
///
/// XCH: member-nonce walk only (default max 100). CAT: market receive-address hints by
/// default; optional `--max-nonce` adds an orphan member walk with always-on empty-batch stop.
fn vault_trace_member_discovery(
    kind: VaultTraceAssetKind,
    max_nonce: Option<u32>,
    cat_hint_puzzle_hashes: Vec<String>,
) -> SignerResult<MemberDiscovery> {
    match kind {
        VaultTraceAssetKind::Xch => Ok(MemberDiscovery::nonces(max_nonce.unwrap_or(100))),
        VaultTraceAssetKind::Cat => match max_nonce {
            None => {
                if cat_hint_puzzle_hashes.is_empty() {
                    return Err(SignerError::Other(
                        "vault-asset-trace CAT path needs a market receive_address for the asset, \
                         or pass --max-nonce N to scan vault member nonces"
                            .to_string(),
                    ));
                }
                Ok(MemberDiscovery::Hints {
                    puzzle_hashes: cat_hint_puzzle_hashes,
                })
            }
            Some(max_nonce) => Ok(MemberDiscovery::HintsThenNonces {
                puzzle_hashes: cat_hint_puzzle_hashes,
                max_nonce,
                empty_batch_stop: EmptyBatchStop::Always,
            }),
        },
    }
}

fn market_matches_cat_asset(
    ticker_index: &CatTickerIndex,
    market: &MarketConfig,
    resolved_asset_id: &str,
    requested_asset: &str,
) -> bool {
    let requested = requested_asset.trim().to_ascii_lowercase();
    for label in [market.base_asset.as_str(), market.base_symbol.as_str()] {
        if label.trim().to_ascii_lowercase() == requested {
            return true;
        }
        if ticker_index.label_refers_to_asset(label, resolved_asset_id) {
            return true;
        }
    }
    false
}

/// Collect unique receive p2 hashes for markets whose base matches the CAT asset.
fn cat_receive_hint_puzzle_hashes(
    markets: &[MarketConfig],
    ticker_index: &CatTickerIndex,
    resolved_asset_id: &str,
    requested_asset: &str,
) -> SignerResult<Vec<String>> {
    let resolved = normalize_hex_id(resolved_asset_id);
    let mut hashes = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for market in markets {
        if !market_matches_cat_asset(ticker_index, market, &resolved, requested_asset)
            || market.receive_address.trim().is_empty()
        {
            continue;
        }
        let hash = normalize_hex_id(&puzzle_hash_hex_for_receive_address(
            &market.receive_address,
        )?);
        if !hash.is_empty() && seen.insert(hash.clone()) {
            hashes.push(hash);
        }
    }
    Ok(hashes)
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

    let cat_hints = if matches!(resolved_asset.kind, VaultTraceAssetKind::Cat) {
        let markets =
            load_markets_config_with_overlay(&mgr.markets_config, mgr.testnet_markets_path())?;
        cat_receive_hint_puzzle_hashes(
            &markets.markets,
            &ticker_index,
            &resolved_asset.asset_id,
            request.asset,
        )?
    } else {
        Vec::new()
    };
    let discovery =
        vault_trace_member_discovery(resolved_asset.kind, request.max_nonce, cat_hints)?;

    let mut scan_params = manager_vault_scan_params(
        mgr,
        &coinset,
        &launcher.launcher_id,
        discovery,
        true,
        resolved_asset.kind.scan_asset_type(),
        resolved_asset
            .kind
            .scan_cat_asset_id(&resolved_asset.asset_id),
    );
    scan_params.start_height = request.start_height;
    let scan = run_manager_vault_scan(scan_params).await?;

    let trace = build_asset_trace(
        &resolved_asset.asset_id,
        resolved_asset.kind.json_label(),
        &scan.coins,
    );

    mgr.emit_json(&trace_payload(&scan, &trace, request.asset)?)?;
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
        start_height,
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
        start_height,
        asset: asset.as_str(),
    }))
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager_cli::vault_scan_sim::sim_dust_scan_result;
    use crate::vault_coinset_scan::build_asset_trace;
    use serde_json::json;
    use std::collections::{HashMap, HashSet};

    #[test]
    fn trace_payload_from_sim_scan_matches_manager_contract() {
        let (scan, _) = sim_dust_scan_result(&[1000, 2000]);
        let asset_id = scan
            .coins
            .first()
            .and_then(|row| row.cat_asset_id.as_deref())
            .expect("cat asset id");
        let trace = build_asset_trace(asset_id, VaultTraceAssetKind::Cat.json_label(), &scan.coins);
        let payload = trace_payload(&scan, &trace, asset_id).expect("payload");

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
        assert_eq!(payload.get("lineage_coin_count"), Some(&json!(2)));
        assert_eq!(
            payload
                .get("current_balance")
                .and_then(|value| value.get("unspent_coin_count")),
            Some(&json!(2))
        );
        assert_eq!(
            payload
                .get("scan")
                .and_then(|value| value.get("scanned_row_count")),
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
        assert!(payload.get("coin_count").is_none());
        let coins = payload
            .get("coins")
            .and_then(|v| v.as_array())
            .expect("coins");
        assert!(coins
            .iter()
            .all(|coin| coin.get("co_input_coin_ids").is_none()));
        let chains = payload
            .get("chains")
            .and_then(|v| v.as_array())
            .expect("chains");
        assert!(chains
            .iter()
            .all(|chain| chain.get("reception_coin_id").is_none()));
    }

    #[test]
    fn xch_discovery_is_nonce_walk_without_hints() {
        let plan = vault_trace_member_discovery(
            VaultTraceAssetKind::Xch,
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
        let plan = vault_trace_member_discovery(VaultTraceAssetKind::Cat, None, hashes.clone())
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
        let err = vault_trace_member_discovery(VaultTraceAssetKind::Cat, None, Vec::new())
            .expect_err("needs hints or max-nonce");
        assert!(err.to_string().contains("receive_address"));
    }

    #[test]
    fn cat_discovery_with_max_nonce_uses_hints_then_nonces() {
        let hashes = vec!["aa".repeat(32)];
        let plan = vault_trace_member_discovery(VaultTraceAssetKind::Cat, Some(7), hashes.clone())
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

    #[test]
    fn market_matches_cat_asset_via_base_symbol() {
        let asset_id = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let mut by_ticker = HashMap::new();
        by_ticker.insert("byc".to_string(), HashSet::from([asset_id.to_string()]));
        let index = CatTickerIndex {
            by_ticker,
            symbols_by_asset_id: std::collections::BTreeMap::default(),
        };
        let market = MarketConfig {
            market_id: "m".to_string(),
            enabled: true,
            unique_maker_coins: true,
            base_asset: "unrelated".to_string(),
            base_symbol: "BYC".to_string(),
            quote_asset: "xch".to_string(),
            quote_asset_type: "volatile".to_string(),
            receive_address: String::new(),
            signer_key_id: "k".to_string(),
            mode: "one_sided".to_string(),
            pricing: crate::config::MarketPricing::default(),
            cancel_move_threshold_bps: None,
            ladders: HashMap::new(),
        };
        assert!(market_matches_cat_asset(&index, &market, asset_id, "other"));
    }

    #[test]
    fn cat_receive_hints_match_ticker_market_when_operator_passes_asset_id() {
        let asset_id = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let mut by_ticker = HashMap::new();
        by_ticker.insert("byc".to_string(), HashSet::from([asset_id.to_string()]));
        let index = CatTickerIndex {
            by_ticker,
            symbols_by_asset_id: std::collections::BTreeMap::default(),
        };
        let receive = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h".to_string();
        let expected =
            normalize_hex_id(&puzzle_hash_hex_for_receive_address(&receive).expect("receive p2"));
        let market = MarketConfig {
            market_id: "byc-xch".to_string(),
            enabled: true,
            unique_maker_coins: true,
            base_asset: "BYC".to_string(),
            base_symbol: "BYC".to_string(),
            quote_asset: "xch".to_string(),
            quote_asset_type: "volatile".to_string(),
            receive_address: receive,
            signer_key_id: "k".to_string(),
            mode: "one_sided".to_string(),
            pricing: crate::config::MarketPricing::default(),
            cancel_move_threshold_bps: None,
            ladders: HashMap::new(),
        };

        let hashes =
            cat_receive_hint_puzzle_hashes(&[market], &index, asset_id, asset_id).expect("hints");
        assert_eq!(hashes, vec![expected]);
    }
}
