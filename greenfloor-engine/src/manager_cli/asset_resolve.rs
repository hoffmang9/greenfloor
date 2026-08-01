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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manager_cli::test_support::{
        write_combine_dust_cats, write_combine_dust_markets, ManagerContextBuilder,
    };
    use crate::minimal_program_template::{
        write_minimal_program_with_signer, MinimalProgramParams,
    };

    const CAT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const RECEIVE: &str = "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h";

    fn harness_with_market_and_cats() -> (
        tempfile::TempDir,
        crate::manager_cli::context::ManagerContext,
    ) {
        let dir = tempfile::tempdir().expect("tmp");
        let program = dir.path().join("program.yaml");
        let markets = dir.path().join("markets.yaml");
        let cats = dir.path().join("cats.yaml");
        write_minimal_program_with_signer(
            &program,
            MinimalProgramParams {
                home_dir: dir.path(),
                ..Default::default()
            },
        );
        write_combine_dust_markets(&markets, CAT, RECEIVE);
        let mut yaml = std::fs::read_to_string(&markets).expect("read");
        yaml = yaml.replace("dust_m", "m1");
        std::fs::write(&markets, yaml).expect("rewrite");
        write_combine_dust_cats(&cats, CAT);
        let harness = ManagerContextBuilder::new(program, markets)
            .cats_config(cats)
            .scratch_dir(dir.path().to_path_buf())
            .build_capturing();
        (dir, harness.ctx)
    }

    #[tokio::test]
    async fn resolve_market_base_cat_and_orphan_helpers() {
        let (_dir, ctx) = harness_with_market_and_cats();
        let bundle = crate::config::load_program_bundle_gated(&ctx.program_config).expect("bundle");

        let (asset, market) = resolve_market_base_cat_asset(&ctx, "mainnet", "m1", Some(CAT))
            .await
            .expect("market base");
        assert_eq!(asset, CAT);
        assert_eq!(market, "m1");

        let err = resolve_market_base_cat_asset(&ctx, "mainnet", "m1", Some(&"bb".repeat(32)))
            .await
            .expect_err("mismatch");
        assert!(err.to_string().contains("does not match"));

        let (orphan_asset, orphan_market) =
            resolve_orphan_presplit_cat_asset(&ctx, &bundle.signer, "mainnet", Some("m1"), None)
                .await
                .expect("orphan market");
        assert_eq!(orphan_asset, CAT);
        assert_eq!(orphan_market.as_deref(), Some("m1"));

        let (hex_asset, none_market) =
            resolve_orphan_presplit_cat_asset(&ctx, &bundle.signer, "mainnet", None, Some(CAT))
                .await
                .expect("orphan hex");
        assert_eq!(hex_asset, CAT);
        assert!(none_market.is_none());

        let err = resolve_orphan_presplit_cat_asset(&ctx, &bundle.signer, "mainnet", None, None)
            .await
            .expect_err("missing");
        assert!(err.to_string().contains("provide --asset"));

        let vault_asset = resolve_cat_vault_trace_asset(&ctx, &bundle.signer, "mainnet", CAT)
            .await
            .expect("vault trace");
        assert_eq!(vault_asset, CAT);

        let err = resolve_cat_vault_trace_asset(&ctx, &bundle.signer, "mainnet", "xch")
            .await
            .expect_err("xch");
        assert!(err.to_string().contains("must resolve to a CAT"));
    }

    #[tokio::test]
    async fn resolve_market_inventory_asset_id_override_and_default() {
        let (_dir, ctx) = harness_with_market_and_cats();
        let loaded = load_gated_operator_market(&GatedOperatorMarketLoadRequest {
            program_path: &ctx.program_config,
            markets_path: &ctx.markets_config,
            testnet_markets_path: ctx.testnet_markets_path(),
            cats_path: Some(&ctx.cats_config),
            network: "mainnet",
            market_id: Some("m1"),
            pair: None,
            command: OperatorMarketCommand::CoinList,
        })
        .expect("load");
        let resolver = loaded.asset_resolver();
        let default_id =
            resolve_market_inventory_asset_id(&resolver, &loaded.market_row.base_asset, None)
                .await
                .expect("default");
        assert_eq!(normalize_hex_id(&default_id), CAT);
        let override_id =
            resolve_market_inventory_asset_id(&resolver, &loaded.market_row.base_asset, Some(CAT))
                .await
                .expect("override");
        assert_eq!(normalize_hex_id(&override_id), CAT);
    }
}
