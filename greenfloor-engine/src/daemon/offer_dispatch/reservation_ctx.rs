use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

use crate::coinset::is_xch_like_asset;
use crate::config::{load_program_config, MarketConfig};
use crate::cycle::ParallelReservationContext;
use crate::error::{SignerError, SignerResult};
use crate::offer::build_context::resolve_quote_price_for_pricing;
use crate::offer::resolve_offer_assets_for_action;
use crate::config::load_signer_config;

pub fn reservation_wallet_id(program_path: &Path) -> SignerResult<String> {
    let raw = std::fs::read_to_string(program_path).map_err(|err| {
        SignerError::Other(format!("failed to read program config: {err}"))
    })?;
    let parsed: Value = serde_yaml::from_str(&raw)
        .map_err(|err| SignerError::Other(format!("failed to parse program config: {err}")))?;
    let launcher_id = parsed
        .get("vault")
        .and_then(|vault| vault.get("launcher_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if !launcher_id.is_empty() {
        return Ok(launcher_id.to_string());
    }
    Ok("signer".to_string())
}

pub async fn parallel_reservation_context(
    program_path: &Path,
    market: &MarketConfig,
    fee_amount_mojos: i64,
) -> SignerResult<ParallelReservationContext> {
    let signer_config = load_signer_config(program_path)?;
    let program = load_program_config(program_path)?;
    let quote_asset = crate::config::resolve_quote_asset_for_offer(
        market.quote_asset.trim(),
        &program.network,
    );
    let (base_asset_id, quote_asset_id) =
        resolve_offer_assets_for_action(&signer_config, market.base_asset.trim(), &quote_asset)
            .await?;
    let fee_asset_id = if is_xch_like_asset(&quote_asset) {
        quote_asset_id.clone()
    } else {
        resolve_offer_assets_for_action(&signer_config, "xch", &quote_asset)
            .await?
            .0
    };
    let base_unit_mojo_multiplier = market
        .pricing
        .get("base_unit_mojo_multiplier")
        .and_then(|value| value.as_i64())
        .unwrap_or(1000);
    let quote_unit_mojo_multiplier = market
        .pricing
        .get("quote_unit_mojo_multiplier")
        .and_then(|value| value.as_i64())
        .unwrap_or(1000);
    let quote_price = resolve_quote_price_for_pricing(&market.pricing)?;
    Ok(ParallelReservationContext {
        base_asset_id: base_asset_id.trim().to_string(),
        quote_asset_id: quote_asset_id.trim().to_string(),
        fee_asset_id: fee_asset_id.trim().to_string(),
        fee_amount_mojos,
        base_unit_mojo_multiplier,
        quote_unit_mojo_multiplier,
        quote_price,
    })
}

pub fn parallel_reservation_asset_ids(ctx: &ParallelReservationContext) -> BTreeSet<String> {
    [ctx.base_asset_id.clone(), ctx.quote_asset_id.clone(), ctx.fee_asset_id.clone()]
        .into_iter()
        .filter(|asset_id| !asset_id.trim().is_empty())
        .collect()
}
