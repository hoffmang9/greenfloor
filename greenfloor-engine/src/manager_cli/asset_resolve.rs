//! Shared manager helpers for resolving CAT inventory / vault-trace assets.

use crate::config::{
    load_gated_operator_market, operator_ticker_index_from_paths, GatedOperatorMarketLoadRequest,
    OperatorMarketCommand, SignerConfig,
};
use crate::error::{SignerError, SignerResult};
use crate::hex::{is_hex_id, normalize_hex_id};
use crate::manager_cli::context::ManagerContext;
use crate::offer::{OfferAssetResolver, VaultTraceAssetKind};

/// Resolve a market's `base_asset` to a normalized CAT inventory id.
///
/// When `asset_filter` is set it must resolve to the same id (guard against mismatched `--asset`).
///
/// # Errors
///
/// Returns an error when the market cannot be loaded, `base_asset` is not a CAT hex id, or
/// `asset_filter` does not match the market base.
pub async fn resolve_market_base_cat_asset(
    ctx: &ManagerContext,
    network: &str,
    market_id: &str,
    asset_filter: Option<&str>,
) -> SignerResult<(String, String)> {
    let loaded = load_gated_operator_market(&GatedOperatorMarketLoadRequest {
        program_path: &ctx.program_config,
        markets_path: &ctx.markets_config,
        testnet_markets_path: ctx.testnet_markets_path(),
        cats_path: Some(&ctx.cats_config),
        network,
        market_id: Some(market_id),
        pair: None,
        command: OperatorMarketCommand::CoinList,
    })?;
    let resolver = loaded.asset_resolver();
    let base = resolver
        .resolve_inventory_asset(loaded.market_row.base_asset.trim())
        .await?;
    if !is_hex_id(&base) {
        return Err(SignerError::Other(
            "market base_asset must resolve to a CAT asset id".to_string(),
        ));
    }
    let base = normalize_hex_id(&base);
    if let Some(filter) = asset_filter.map(str::trim).filter(|v| !v.is_empty()) {
        let inventory_asset = resolver.resolve_inventory_asset(filter).await?;
        if normalize_hex_id(&inventory_asset) != base {
            return Err(SignerError::Other(
                "--asset does not match market base_asset".to_string(),
            ));
        }
    }
    Ok((base, loaded.market_row.market_id))
}

/// Resolve a ticker / asset filter to a normalized CAT vault-trace asset id (no market).
///
/// # Errors
///
/// Returns an error when the filter does not resolve to a CAT.
pub async fn resolve_cat_vault_trace_asset(
    ctx: &ManagerContext,
    signer: &SignerConfig,
    network: &str,
    asset: &str,
) -> SignerResult<String> {
    let ticker_index = operator_ticker_index_from_paths(
        &ctx.markets_config,
        ctx.testnet_markets_path(),
        Some(&ctx.cats_config),
    );
    let vault_asset = OfferAssetResolver::new(signer, &ticker_index, network)
        .resolve_vault_trace_asset(asset)
        .await?;
    if !matches!(vault_asset.kind, VaultTraceAssetKind::Cat) {
        return Err(SignerError::Other(
            "asset must resolve to a CAT".to_string(),
        ));
    }
    Ok(normalize_hex_id(&vault_asset.asset_id))
}

/// Resolve CAT asset id for orphan-presplit: market base and/or `--asset` ticker/hex.
///
/// Returns `(normalized_asset_id, optional_market_id)`.
///
/// # Errors
///
/// Returns an error when neither input is provided or resolution fails.
pub async fn resolve_orphan_presplit_cat_asset(
    ctx: &ManagerContext,
    signer: &SignerConfig,
    network: &str,
    market_id: Option<&str>,
    asset: Option<&str>,
) -> SignerResult<(String, Option<String>)> {
    if let Some(mid) = market_id.map(str::trim).filter(|v| !v.is_empty()) {
        let (asset_id, market) = resolve_market_base_cat_asset(ctx, network, mid, asset)
            .await
            .map_err(map_orphan_asset_err)?;
        return Ok((asset_id, Some(market)));
    }
    let Some(filter) = asset.map(str::trim).filter(|v| !v.is_empty()) else {
        return Err(SignerError::Other(
            "provide --asset and/or --market-id".to_string(),
        ));
    };
    let asset_id = resolve_cat_vault_trace_asset(ctx, signer, network, filter)
        .await
        .map_err(|err| match err {
            SignerError::Other(msg) if msg.contains("must resolve to a CAT") => {
                SignerError::Other("offers-orphan-presplit requires a CAT asset".to_string())
            }
            other => other,
        })?;
    Ok((asset_id, None))
}

fn map_orphan_asset_err(err: SignerError) -> SignerError {
    match err {
        SignerError::Other(msg) if msg.contains("must resolve to a CAT asset id") => {
            SignerError::Other(
                "offers-orphan-presplit requires a CAT market base_asset".to_string(),
            )
        }
        other => other,
    }
}

/// Resolve inventory asset id for a loaded market (optional `--asset` override).
///
/// # Errors
///
/// Returns an error when inventory resolution fails.
pub async fn resolve_market_inventory_asset_id(
    resolver: &OfferAssetResolver<'_>,
    market_base_asset: &str,
    asset_filter: Option<&str>,
) -> SignerResult<String> {
    if let Some(filter) = asset_filter.map(str::trim).filter(|v| !v.is_empty()) {
        resolver.resolve_inventory_asset(filter).await
    } else {
        Ok(market_base_asset.to_string())
    }
}
