//! Manager CLI entrypoints for offers status, reconcile, cancel, and presplit reclaim/orphan.

use clap::Args;

use crate::cli_util::{optional_str, optional_trimmed};
use crate::coinset::resolve_coinset_endpoint;
use crate::config::{
    load_gated_operator_market, load_program_bundle_gated, load_program_config,
    operator_ticker_index_from_paths, GatedOperatorMarketLoadRequest, OperatorMarketCommand,
};
use crate::error::{SignerError, SignerResult};
use crate::hex::normalize_hex_id;
use crate::offer::lifecycle::{
    offers_cancel_cli, offers_orphan_presplit_cli, offers_reclaim_presplit_cli, offers_status_cli,
    reconcile_offers_cli, OffersCancelCliRequest, OffersCancelCliResult,
    OffersOrphanPresplitCliRequest, OffersOrphanPresplitCliResult, OffersReclaimPresplitCliResult,
};
use crate::offer::presplit::OrphanScanRow;
use crate::offer::{OfferAssetResolver, VaultTraceAssetKind};
use crate::storage::resolve_state_db_path;
use crate::vault_coinset_scan::types::{AssetTypeFilter, CoinRow};

use super::context::ManagerContext;
use super::vault_scan::{
    manager_vault_scan_params, resolve_manager_vault_launcher, run_manager_vault_scan,
};

#[derive(Debug, Args)]
pub struct OffersReconcileCliArgs {
    #[arg(long, default_value = "")]
    pub market_id: String,
    #[arg(long, default_value_t = 200)]
    pub limit: usize,
    #[arg(long)]
    pub venue: Option<String>,
}

#[derive(Debug, Args)]
pub struct OffersStatusCliArgs {
    #[arg(long, default_value = "")]
    pub market_id: String,
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
    #[arg(long, default_value_t = 30)]
    pub events_limit: usize,
}

#[derive(Debug, Args)]
pub struct OffersCancelCliArgs {
    #[arg(long, action = clap::ArgAction::Append)]
    pub offer_id: Vec<String>,
    #[arg(long, action = clap::ArgAction::Append, value_name = "PATH_OR_BECH32")]
    pub offer_file: Vec<String>,
    #[arg(long, value_name = "MARKET_ID")]
    pub market_id: Option<String>,
    #[arg(long)]
    pub cancel_open: bool,
    #[arg(long)]
    pub venue: Option<String>,
}

#[derive(Debug, Args)]
pub struct OffersReclaimPresplitCliArgs {
    #[arg(long, action = clap::ArgAction::Append)]
    pub coin_id: Vec<String>,
    #[arg(long, action = clap::ArgAction::Append)]
    pub fixed_delegated_puzzle_hash: Vec<String>,
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
pub struct OffersOrphanPresplitCliArgs {
    pub asset: String,
    pub market_id: String,
    pub network: String,
    pub coinset_base_url: String,
    pub launcher_id: String,
    pub launcher_id_file: String,
    pub max_nonce: u32,
    pub start_height: Option<u64>,
    pub reclaim: bool,
    pub dry_run: bool,
    pub no_wait: bool,
}

/// Run offers reconcile command.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_offers_reconcile_command(
    ctx: &ManagerContext,
    args: OffersReconcileCliArgs,
) -> SignerResult<i32> {
    let program = load_program_config(&ctx.program_config)?;
    let db_path = resolve_state_db_path(&program.home_dir, ctx.state_db_override());
    let venue = args
        .venue
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(program.offer_publish_venue.as_str())
        .to_ascii_lowercase();
    let payload = reconcile_offers_cli(
        &db_path,
        &program.dexie_api_base,
        &venue,
        optional_trimmed(&args.market_id).as_deref(),
        args.limit,
    )
    .await?;
    ctx.emit_serialized(&payload)?;
    Ok(0)
}

/// Run offers status command.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn run_offers_status_command(
    ctx: &ManagerContext,
    args: &OffersStatusCliArgs,
) -> SignerResult<i32> {
    let program = load_program_config(&ctx.program_config)?;
    let db_path = resolve_state_db_path(&program.home_dir, ctx.state_db_override());
    let payload = offers_status_cli(
        &db_path,
        optional_trimmed(&args.market_id).as_deref(),
        args.limit,
        args.events_limit,
    )?;
    ctx.emit_serialized(&payload)?;
    Ok(0)
}

/// Run offers cancel command.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_offers_cancel_command(
    ctx: &ManagerContext,
    args: OffersCancelCliArgs,
) -> SignerResult<i32> {
    let bundle = load_program_bundle_gated(&ctx.program_config)?;
    let program = bundle.program;
    let db_path = resolve_state_db_path(&program.home_dir, None);
    let venue = args
        .venue
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(program.offer_publish_venue.as_str())
        .to_ascii_lowercase();
    let payload: OffersCancelCliResult = offers_cancel_cli(
        &db_path,
        &program.dexie_api_base,
        &venue,
        &OffersCancelCliRequest {
            offer_ids: args.offer_id,
            offer_files: args.offer_file,
            market_id: args.market_id,
            cancel_open: args.cancel_open,
        },
        bundle.signer,
        &program.network,
    )
    .await?;
    let exit_code = if payload.failed_count == 0 { 0 } else { 2 };
    ctx.emit_serialized(&payload)?;
    Ok(exit_code)
}

/// Run orphaned-presplit reclaim command (metadata cancel; no `offer_state` row).
///
/// # Errors
///
/// Returns an error if pair parsing or program/signer load fails.
pub async fn run_offers_reclaim_presplit_command(
    ctx: &ManagerContext,
    args: OffersReclaimPresplitCliArgs,
) -> SignerResult<i32> {
    let bundle = load_program_bundle_gated(&ctx.program_config)?;
    let program = bundle.program;
    let payload: OffersReclaimPresplitCliResult = offers_reclaim_presplit_cli(
        bundle.signer,
        &program.network,
        &args.coin_id,
        &args.fixed_delegated_puzzle_hash,
        args.dry_run,
    )
    .await?;
    let exit_code = if payload.failed_count == 0 { 0 } else { 2 };
    ctx.emit_serialized(&payload)?;
    Ok(exit_code)
}

fn orphan_scan_rows_from_vault_coins(coins: &[CoinRow]) -> Vec<OrphanScanRow> {
    coins
        .iter()
        .filter(|row| row.kind.is_cat())
        .map(|row| OrphanScanRow {
            coin_id: row.coin_id.clone(),
            parent_coin_info: row.parent_coin_info.clone(),
            amount: row.amount,
            confirmed_block_index: row.confirmed_block_index,
            spent_block_index: row.spent_block_index,
            discovered_by_hint: row.discovered_by_hint,
            discovered_by_puzzle_hash: row.discovered_by_puzzle_hash,
            cat_asset_id: row.cat_asset_id.clone(),
        })
        .collect()
}

async fn resolve_orphan_presplit_asset(
    ctx: &ManagerContext,
    signer: &crate::config::SignerConfig,
    network: &str,
    market_id: Option<&str>,
    asset: Option<&str>,
) -> SignerResult<(String, Option<String>)> {
    if let Some(mid) = market_id {
        let loaded = load_gated_operator_market(&GatedOperatorMarketLoadRequest {
            program_path: &ctx.program_config,
            markets_path: &ctx.markets_config,
            testnet_markets_path: ctx.testnet_markets_path(),
            cats_path: Some(&ctx.cats_config),
            network,
            market_id: Some(mid),
            pair: None,
            command: OperatorMarketCommand::CoinList,
        })?;
        let resolver = loaded.asset_resolver();
        let base = resolver
            .resolve_inventory_asset(loaded.market_row.base_asset.trim())
            .await?;
        if !crate::hex::is_hex_id(&base) {
            return Err(SignerError::Other(
                "offers-orphan-presplit requires a CAT market base_asset".to_string(),
            ));
        }
        if let Some(filter) = asset {
            let inventory_asset = resolver.resolve_inventory_asset(filter).await?;
            if normalize_hex_id(&inventory_asset) != normalize_hex_id(&base) {
                return Err(SignerError::Other(
                    "--asset does not match market base_asset".to_string(),
                ));
            }
        }
        return Ok((normalize_hex_id(&base), Some(mid.to_string())));
    }
    let Some(filter) = asset else {
        return Err(SignerError::Other(
            "provide --asset and/or --market-id".to_string(),
        ));
    };
    let ticker_index = operator_ticker_index_from_paths(
        &ctx.markets_config,
        ctx.testnet_markets_path(),
        Some(&ctx.cats_config),
    );
    let vault_asset = OfferAssetResolver::new(signer, &ticker_index, network)
        .resolve_vault_trace_asset(filter)
        .await?;
    if !matches!(vault_asset.kind, VaultTraceAssetKind::Cat) {
        return Err(SignerError::Other(
            "offers-orphan-presplit requires a CAT asset".to_string(),
        ));
    }
    Ok((normalize_hex_id(&vault_asset.asset_id), None))
}

/// Discover vault-hinted orphaned presplit makers; optionally reclaim recoverable ones.
///
/// # Errors
///
/// Returns an error when config, vault scan, discovery, or reclaim fails.
pub async fn run_offers_orphan_presplit_command(
    ctx: &ManagerContext,
    args: OffersOrphanPresplitCliArgs,
) -> SignerResult<i32> {
    let bundle = load_program_bundle_gated(&ctx.program_config)?;
    let network = optional_str(&args.network).unwrap_or(bundle.program.network.as_str());
    let (asset_id, market_id_out) = resolve_orphan_presplit_asset(
        ctx,
        &bundle.signer,
        network,
        optional_trimmed(&args.market_id).as_deref(),
        optional_trimmed(&args.asset).as_deref(),
    )
    .await?;

    let coinset = resolve_coinset_endpoint(
        network,
        &bundle.signer.coinset_base_url,
        optional_str(&args.coinset_base_url),
    );
    let launcher = resolve_manager_vault_launcher(
        ctx,
        optional_str(&args.launcher_id),
        optional_str(&args.launcher_id_file),
    )?;
    let mut scan_params = manager_vault_scan_params(
        ctx,
        &coinset,
        &launcher.launcher_id,
        args.max_nonce,
        false,
        AssetTypeFilter::Cat,
        Some(asset_id.as_str()),
    );
    scan_params.start_height = args.start_height;
    let scan = run_manager_vault_scan(scan_params).await?;

    let payload: OffersOrphanPresplitCliResult = offers_orphan_presplit_cli(
        bundle.signer,
        network,
        OffersOrphanPresplitCliRequest {
            asset_id,
            market_id: market_id_out,
            start_height: args.start_height,
            reclaim: args.reclaim,
            dry_run: args.dry_run,
            wait: !args.no_wait,
            home_dir: bundle.program.home_dir.clone(),
            state_db_override: ctx.state_db_override().map(str::to_string),
            launcher_id: launcher.launcher_id,
            scan_rows: orphan_scan_rows_from_vault_coins(&scan.coins),
        },
    )
    .await?;
    let failed = payload
        .reclaim_result
        .as_ref()
        .is_some_and(|result| result.failed_count > 0);
    let exit_code = if failed { 2 } else { 0 };
    ctx.emit_serialized(&payload)?;
    Ok(exit_code)
}

#[cfg(test)]
mod tests;
