use serde_json::{json, Value};

use crate::coin_ops::coin_op_non_negative_u64;
use crate::coin_ops::combine_output_amounts;
use crate::config::SignerConfig;
use crate::error::SignerResult;
use crate::offer::bootstrap::{BootstrapFundingSource, BootstrapPlan};
use crate::vault::{build_and_optionally_broadcast_vault_cat_mixed_split, MixedSplitRequest};

async fn submit_bootstrap_vault_mixed_split(
    signer_config: &SignerConfig,
    split_asset_id: &str,
    receive_address: &str,
    coin_ids: &[String],
    output_amounts_mojos: Vec<u64>,
    #[cfg(test)] test_overrides: Option<&super::test_overrides::SignerDenominationTestOverrides>,
) -> SignerResult<Value> {
    #[cfg(test)]
    if let Some(stub) = super::test_overrides::vault_mixed_split_stub_response(test_overrides) {
        let _ = (
            signer_config,
            split_asset_id,
            receive_address,
            coin_ids,
            output_amounts_mojos,
        );
        return Ok(stub);
    }
    let result = build_and_optionally_broadcast_vault_cat_mixed_split(
        signer_config.clone(),
        MixedSplitRequest {
            receive_address: receive_address.to_string(),
            asset_id: crate::hex::hex_to_bytes32(split_asset_id)?,
            output_amounts: output_amounts_mojos,
            coin_ids: crate::coinset::parse_coin_ids(coin_ids)?,
            allow_sub_cat_output: false,
            fee_mojos: 0,
        },
        true,
    )
    .await?;
    Ok(json!({
        "offered_total": result.offered_total,
        "target_total": result.target_total,
        "change_amount": result.change_amount,
        "selected_coin_ids": result.selected_coin_ids,
        "broadcast_status": result.broadcast_status,
        "spend_bundle_hex": result.spend_bundle_hex,
    }))
}

pub(super) async fn submit_bootstrap_combine(
    signer_config: &SignerConfig,
    bootstrap_plan: &BootstrapPlan,
    split_asset_id: &str,
    receive_address: &str,
    split_asset_mojo_multiplier: i64,
    #[cfg(test)] test_overrides: Option<&super::test_overrides::SignerDenominationTestOverrides>,
) -> SignerResult<Value> {
    let BootstrapFundingSource::CombineFirst(inputs) = &bootstrap_plan.funding else {
        return Err(crate::error::SignerError::InvalidPlanValues);
    };
    let multiplier = split_asset_mojo_multiplier.max(1);
    let total_mojos = inputs.selected_total.saturating_mul(multiplier);
    let output_amounts = combine_output_amounts(total_mojos, 1)?;
    let mut result = submit_bootstrap_vault_mixed_split(
        signer_config,
        split_asset_id,
        receive_address,
        &inputs.input_coin_ids,
        output_amounts,
        #[cfg(test)]
        test_overrides,
    )
    .await?;
    if let Some(obj) = result.as_object_mut() {
        obj.insert(
            "input_coin_count".to_string(),
            json!(inputs.input_coin_ids.len()),
        );
    }
    Ok(result)
}

pub(super) async fn submit_bootstrap_mixed_split(
    signer_config: &SignerConfig,
    bootstrap_plan: &BootstrapPlan,
    split_asset_id: &str,
    receive_address: &str,
    split_asset_mojo_multiplier: i64,
    #[cfg(test)] test_overrides: Option<&super::test_overrides::SignerDenominationTestOverrides>,
) -> SignerResult<Value> {
    let BootstrapFundingSource::SingleCoin { coin_id, .. } = &bootstrap_plan.funding else {
        return Err(crate::error::SignerError::InvalidPlanValues);
    };
    let multiplier = split_asset_mojo_multiplier.max(1);
    let output_amounts_mojos = bootstrap_plan
        .output_amounts_base_units
        .iter()
        .map(|amount| {
            coin_op_non_negative_u64(
                amount.saturating_mul(multiplier),
                "bootstrap.output_amount_mojos",
            )
        })
        .collect::<SignerResult<Vec<_>>>()?;
    submit_bootstrap_vault_mixed_split(
        signer_config,
        split_asset_id,
        receive_address,
        std::slice::from_ref(coin_id),
        output_amounts_mojos,
        #[cfg(test)]
        test_overrides,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::submit_bootstrap_mixed_split;
    use crate::offer::bootstrap::{BootstrapFundingSource, BootstrapPlan};
    use crate::test_support::signer_config::test_signer_config;

    fn sample_split_plan(source_coin_id: &str) -> BootstrapPlan {
        BootstrapPlan {
            funding: BootstrapFundingSource::SingleCoin {
                coin_id: source_coin_id.to_string(),
                amount: 1_000,
            },
            output_amounts_base_units: vec![100],
            total_output_amount: 100,
            change_amount: 900,
            deficits: Vec::new(),
        }
    }

    #[tokio::test]
    async fn submit_bootstrap_mixed_split_rejects_invalid_asset_hex() {
        let signer = test_signer_config("https://example.test");
        let plan = sample_split_plan(&"aa".repeat(64));

        let err = submit_bootstrap_mixed_split(
            &signer,
            &plan,
            "not-a-valid-asset-id",
            "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
            1,
            None,
        )
        .await
        .expect_err("invalid asset hex");

        assert!(err.to_string().contains("hex"));
    }

    #[tokio::test]
    async fn submit_bootstrap_mixed_split_rejects_invalid_source_coin_id() {
        let signer = test_signer_config("https://example.test");
        let plan = sample_split_plan("not-a-valid-coin-id");

        let err = submit_bootstrap_mixed_split(
            &signer,
            &plan,
            &"aa".repeat(64),
            "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
            1,
            None,
        )
        .await
        .expect_err("invalid coin id");

        assert!(err.to_string().contains("hex"));
    }
}
