use serde_json::{json, Value};

#[cfg(test)]
use super::test_overrides::SignerDenominationTestOverrides;
use crate::coin_ops::shape::ShapeFunding;
use crate::error::SignerResult;
use crate::hex::{hex_to_bytes32, parse_coin_ids};
use crate::offer::bootstrap::{
    bootstrap_combine_vault_outputs_as_mojos, vault_output_mojos_from_plan_amounts, BootstrapPlan,
    PlanAmount,
};
use crate::offer::operator::build_and_post::ResolvedBuildAndPostContext;
use crate::vault::{
    build_and_optionally_broadcast_vault_cat_mixed_split, MixedSplitRequest, MixedSplitResult,
};

fn mixed_split_result_json(result: &MixedSplitResult) -> Value {
    json!({
        "offered_total": result.offered_total,
        "target_total": result.target_total,
        "change_amount": result.change_amount,
        "selected_coin_ids": result.selected_coin_ids,
        "broadcast_status": result.broadcast_status,
        "spend_bundle_hex": result.spend_bundle_hex,
    })
}

async fn submit_bootstrap_vault_mixed_split(
    build_ctx: &ResolvedBuildAndPostContext,
    split_asset_id: &str,
    receive_address: &str,
    coin_ids: &[String],
    output_amounts_mojos: Vec<u64>,
    #[cfg(test)] test_overrides: &SignerDenominationTestOverrides,
) -> SignerResult<Value> {
    #[cfg(test)]
    if let Some(stub) = test_overrides.take_vault_mixed_split_stub(&output_amounts_mojos) {
        let _ = (build_ctx, split_asset_id, receive_address, coin_ids);
        return Ok(mixed_split_result_json(&stub));
    }
    let request = MixedSplitRequest {
        receive_address: receive_address.to_string(),
        asset_id: hex_to_bytes32(split_asset_id)?,
        output_amounts: output_amounts_mojos,
        coin_ids: parse_coin_ids(coin_ids)?,
        allow_sub_cat_output: false,
        fee_mojos: 0,
    };
    let result = build_and_optionally_broadcast_vault_cat_mixed_split(
        build_ctx.gated.signer.clone(),
        &build_ctx.gated.operator_network,
        request,
        true,
    )
    .await
    .map_err(crate::error::SignerError::normalize_mixed_split_error)?;
    Ok(mixed_split_result_json(&result))
}

pub(super) async fn submit_bootstrap_combine(
    build_ctx: &ResolvedBuildAndPostContext,
    bootstrap_plan: &BootstrapPlan,
    split_asset_id: &str,
    receive_address: &str,
    #[cfg(test)] test_overrides: &SignerDenominationTestOverrides,
) -> SignerResult<Value> {
    let ShapeFunding::CombineFirst(inputs) = &bootstrap_plan.funding else {
        return Err(crate::error::SignerError::InvalidPlanValues);
    };
    // Plan amounts are mojos on the denomination path.
    let output_amounts = bootstrap_combine_vault_outputs_as_mojos(inputs)?;
    let mut result = submit_bootstrap_vault_mixed_split(
        build_ctx,
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
    build_ctx: &ResolvedBuildAndPostContext,
    bootstrap_plan: &BootstrapPlan,
    split_asset_id: &str,
    receive_address: &str,
    #[cfg(test)] test_overrides: &SignerDenominationTestOverrides,
) -> SignerResult<Value> {
    let ShapeFunding::SingleCoin { coin_id, .. } = &bootstrap_plan.funding else {
        return Err(crate::error::SignerError::InvalidPlanValues);
    };
    let output_amounts_mojos = vault_output_mojos_from_plan_amounts(
        &bootstrap_plan
            .output_amounts
            .iter()
            .map(|amount| PlanAmount::new(*amount))
            .collect::<Vec<_>>(),
    )?;
    submit_bootstrap_vault_mixed_split(
        build_ctx,
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
    #![allow(clippy::large_futures)]

    use super::{submit_bootstrap_combine, submit_bootstrap_mixed_split};
    use crate::coin_ops::shape::{CombineInputs, ShapeFunding};
    use crate::offer::bootstrap::{
        bootstrap_combine_vault_outputs, bootstrap_combine_vault_outputs_as_mojos, BootstrapPlan,
    };
    use crate::offer::operator::build_and_post::sample_resolved_build_and_post_context;
    use crate::offer::operator::signer_denomination::test_overrides::SignerDenominationTestOverrides;

    fn combine_first_plan(inputs: CombineInputs) -> BootstrapPlan {
        let selected_total = inputs.selected_total;
        let target = inputs.target_amount;
        BootstrapPlan {
            funding: ShapeFunding::CombineFirst(inputs),
            output_amounts: vec![target],
            total_output_amount: target,
            change_amount: selected_total - target,
            deficits: Vec::new(),
        }
    }

    #[test]
    fn bootstrap_combine_vault_outputs_match_eco181_shape() {
        let inputs = CombineInputs {
            input_coin_ids: vec!["a".repeat(64), "b".repeat(64)],
            selected_total: 105,
            target_amount: 100,
            exact_match: false,
            cap_applied: true,
            selected_count_before_cap: 2,
            combine_input_cap: 5,
        };
        let outputs = bootstrap_combine_vault_outputs(&inputs, 1_000).expect("outputs");
        assert_eq!(outputs, vec![100_000]);
        assert_eq!(
            bootstrap_combine_vault_outputs_as_mojos(&inputs).expect("mojos"),
            vec![100]
        );
    }

    #[tokio::test]
    async fn submit_bootstrap_combine_delegates_to_vault_outputs() {
        let overrides = SignerDenominationTestOverrides::default();
        overrides.enqueue_sample_vault_mixed_split_stub();
        let plan = combine_first_plan(CombineInputs {
            input_coin_ids: vec!["a".repeat(64), "b".repeat(64)],
            selected_total: 105_000,
            target_amount: 100_000,
            exact_match: false,
            cap_applied: true,
            selected_count_before_cap: 2,
            combine_input_cap: 5,
        });
        let build_ctx = sample_resolved_build_and_post_context();
        let result = submit_bootstrap_combine(
            &build_ctx,
            &plan,
            &"aa".repeat(64),
            "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
            &overrides,
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
            funding: ShapeFunding::SingleCoin {
                coin_id: source_coin_id.to_string(),
                amount: 1_000,
            },
            output_amounts: vec![100],
            total_output_amount: 100,
            change_amount: 900,
            deficits: Vec::new(),
        }
    }

    #[tokio::test]
    async fn submit_bootstrap_mixed_split_rejects_invalid_asset_hex() {
        let build_ctx = sample_resolved_build_and_post_context();
        let plan = sample_split_plan(&"aa".repeat(64));
        let overrides = SignerDenominationTestOverrides::default();

        let err = submit_bootstrap_mixed_split(
            &build_ctx,
            &plan,
            "not-a-valid-asset-id",
            "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
            &overrides,
        )
        .await
        .expect_err("invalid asset hex");

        assert!(err.to_string().contains("hex"));
    }

    #[tokio::test]
    async fn submit_bootstrap_mixed_split_rejects_invalid_source_coin_id() {
        let build_ctx = sample_resolved_build_and_post_context();
        let plan = sample_split_plan("not-a-valid-coin-id");
        let overrides = SignerDenominationTestOverrides::default();

        let err = submit_bootstrap_mixed_split(
            &build_ctx,
            &plan,
            &"aa".repeat(64),
            "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
            &overrides,
        )
        .await
        .expect_err("invalid coin id");

        assert!(err.to_string().contains("hex"));
    }
}
