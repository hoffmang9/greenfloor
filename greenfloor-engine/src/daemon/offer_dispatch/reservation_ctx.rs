use std::collections::BTreeSet;

use crate::coinset::is_xch_like_asset;
use crate::config::{load_program_config, load_signer_config, MarketConfig};
use crate::cycle::ParallelReservationContext;
use crate::error::SignerResult;
use crate::offer::build_context::resolve_quote_price_for_pricing;
use crate::offer::resolve_offer_assets_for_action;

use crate::daemon::config_paths::DaemonConfigPaths;

pub fn reservation_wallet_id(paths: &DaemonConfigPaths) -> SignerResult<String> {
    let config = load_signer_config(&paths.program_path)?;
    let encoded = hex::encode(config.vault.launcher_id);
    if encoded.is_empty() {
        return Ok("signer".to_string());
    }
    Ok(encoded)
}

pub async fn parallel_reservation_context(
    paths: &DaemonConfigPaths,
    market: &MarketConfig,
    fee_amount_mojos: i64,
) -> SignerResult<ParallelReservationContext> {
    let signer_config = load_signer_config(&paths.program_path)?;
    let program = load_program_config(&paths.program_path)?;
    let quote_asset =
        crate::config::resolve_quote_asset_for_offer(market.quote_asset.trim(), &program.network);
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
    use tempfile::tempdir;

    #[test]
    fn reservation_wallet_id_uses_signer_config_launcher_id() {
        let dir = tempdir().expect("tempdir");
        let program_path = dir.path().join("program.yaml");
        let launcher_id = "aa".repeat(32);
        std::fs::write(
            &program_path,
            format!(
                r#"
app:
  network: testnet11
signer:
  kms_key_id: arn:aws:kms:us-west-2:123:key/abc
  kms_region: us-west-2
vault:
  launcher_id: {launcher_id}
  custody_threshold: 1
  recovery_threshold: 1
  recovery_clawback_timelock: 3600
  custody_keys:
    - public_key_hex: "020202020202020202020202020202020202020202020202020202020202020202"
      curve: SECP256R1
  recovery_keys:
    - public_key_hex: "ab3cb61463a695fa094f7c30526c8097fb813a0c5fa67bab261a7cd354cb9901baa6b7a99d"
      curve: SECP256R1
"#
            ),
        )
        .expect("write");

        let paths = DaemonConfigPaths::new(
            program_path.clone(),
            dir.path().join("markets.yaml"),
            None,
        );
        let wallet_id = reservation_wallet_id(&paths).expect("wallet id");
        assert_eq!(wallet_id, launcher_id);
    }
}
