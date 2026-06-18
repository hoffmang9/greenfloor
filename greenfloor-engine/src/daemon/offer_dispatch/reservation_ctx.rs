use std::collections::BTreeSet;

use crate::coinset::is_xch_like_asset;
use crate::config::{MarketConfig, SignerConfig};
use crate::cycle::ParallelReservationContext;
use crate::error::SignerResult;
use crate::offer::build_context::resolve_quote_price_for_pricing;
use crate::offer::resolve_offer_assets_for_action;

pub fn reservation_wallet_id(signer: &SignerConfig) -> String {
    let encoded = hex::encode(signer.vault.launcher_id);
    if encoded.is_empty() {
        return "signer".to_string();
    }
    encoded
}

pub async fn parallel_reservation_context(
    signer_config: &SignerConfig,
    program_network: &str,
    market: &MarketConfig,
    fee_amount_mojos: i64,
) -> SignerResult<ParallelReservationContext> {
    let quote_asset =
        crate::config::resolve_quote_asset_for_offer(market.quote_asset.trim(), program_network);
    let (base_asset_id, quote_asset_id) =
        resolve_offer_assets_for_action(signer_config, market.base_asset.trim(), &quote_asset)
            .await?;
    let fee_asset_id = if is_xch_like_asset(&quote_asset) {
        quote_asset_id.clone()
    } else {
        resolve_offer_assets_for_action(signer_config, "xch", &quote_asset)
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
    [
        ctx.base_asset_id.clone(),
        ctx.quote_asset_id.clone(),
        ctx.fee_asset_id.clone(),
    ]
    .into_iter()
    .filter(|asset_id| !asset_id.trim().is_empty())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::load_program_bundle;
    use crate::test_support::minimal_program::{
        write_minimal_program_with_signer, MinimalProgramParams,
    };
    use tempfile::tempdir;

    #[test]
    fn reservation_wallet_id_uses_signer_config_launcher_id() {
        let dir = tempdir().expect("tempdir");
        let program_path = dir.path().join("program.yaml");
        write_minimal_program_with_signer(
            &program_path,
            MinimalProgramParams {
                home_dir: dir.path(),
                ..Default::default()
            },
        );
        let bundle = load_program_bundle(&program_path).expect("bundle");
        let wallet_id = reservation_wallet_id(&bundle.signer);
        assert_eq!(wallet_id, "aa".repeat(32));
    }
}
