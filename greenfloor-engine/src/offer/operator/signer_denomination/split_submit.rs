use serde_json::{json, Value};

use crate::config::SignerConfig;
use crate::error::SignerResult;
use crate::offer::bootstrap::{
    bootstrap_combine_vault_outputs, bootstrap_mixed_split_output_mojos, BaseUnits,
    BootstrapFundingSource, BootstrapPlan,
};
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
    if let Some(stub) = super::test_overrides::vault_mixed_split_stub_response(
        test_overrides,
        &output_amounts_mojos,
    ) {
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
    let output_amounts =
        bootstrap_combine_vault_outputs(inputs, split_asset_mojo_multiplier.max(1))?;
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
    let output_amounts_mojos = bootstrap_mixed_split_output_mojos(
        &bootstrap_plan
            .output_amounts_base_units
            .iter()
            .map(|amount| BaseUnits::new(*amount))
            .collect::<Vec<_>>(),
        split_asset_mojo_multiplier.max(1),
    )?;
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
    use super::{submit_bootstrap_combine, submit_bootstrap_mixed_split};
    use crate::offer::bootstrap::{
        bootstrap_combine_vault_outputs, BaseUnits, BootstrapCombineInputs, BootstrapFundingSource,
        BootstrapPlan,
    };
    use crate::offer::operator::signer_denomination::test_overrides::{
        sample_vault_mixed_split_stub, SignerDenominationTestOverrides,
    };
    use crate::test_support::signer_config::test_signer_config;

    fn combine_first_plan(inputs: BootstrapCombineInputs) -> BootstrapPlan {
        let selected_total = inputs.selected_total.get();
        BootstrapPlan {
            funding: BootstrapFundingSource::CombineFirst(inputs),
            output_amounts_base_units: vec![100],
            total_output_amount: 100,
            change_amount: selected_total - 100,
            deficits: Vec::new(),
        }
    }

    #[test]
    fn bootstrap_combine_vault_outputs_match_eco181_shape() {
        let inputs = BootstrapCombineInputs {
            input_coin_ids: vec!["a".repeat(64), "b".repeat(64)],
            selected_total: BaseUnits::new(105),
            target_amount: BaseUnits::new(100),
            exact_match: false,
            cap_applied: true,
        };
        let outputs = bootstrap_combine_vault_outputs(&inputs, 1_000).expect("outputs");
        assert_eq!(outputs, vec![100_000]);
    }

    #[tokio::test]
    async fn submit_bootstrap_combine_delegates_to_vault_outputs() {
        let overrides = SignerDenominationTestOverrides::default();
        overrides.enqueue_vault_mixed_split_stub(sample_vault_mixed_split_stub());
        let plan = combine_first_plan(BootstrapCombineInputs {
            input_coin_ids: vec!["a".repeat(64), "b".repeat(64)],
            selected_total: BaseUnits::new(105),
            target_amount: BaseUnits::new(100),
            exact_match: false,
            cap_applied: true,
        });
        let signer = test_signer_config("https://example.test");
        let result = submit_bootstrap_combine(
            &signer,
            &plan,
            &"aa".repeat(64),
            "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
            1_000,
            Some(&overrides),
        )
        .await
        .expect("combine submit");
        assert_eq!(result["input_coin_count"], 2);
        assert_eq!(
            overrides.take_vault_output_amounts_mojos(),
            Some(vec![100_000])
        );
    }

    fn sample_split_plan(source_coin_id: &str) -> BootstrapPlan {
        BootstrapPlan {
            funding: BootstrapFundingSource::SingleCoin {
                coin_id: source_coin_id.to_string(),
                amount: BaseUnits::new(1_000),
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
