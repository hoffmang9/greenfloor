use std::collections::HashSet;

use serde_json::{json, Value};

use crate::coin_ops::execution::CoinOpExecContext;
#[cfg(test)]
use crate::coin_ops::execution::CoinOpTestOverrides;
use crate::coinset::client_for_signer_on_network;
use crate::error::SignerResult;
use crate::offer::bootstrap::{
    bootstrap_combine_vault_outputs, bootstrap_mixed_split_output_mojos, BaseUnits,
    BootstrapFundingSource, BootstrapPlan,
};
use crate::offer::operator::build_and_post::ResolvedBuildAndPostContext;
use crate::vault::{CatSelection, MixedSplitResult};

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

fn bootstrap_exec_context(
    build_ctx: &ResolvedBuildAndPostContext,
    split_asset_id: &str,
    receive_address: &str,
    #[cfg(test)] test_overrides: CoinOpTestOverrides,
) -> CoinOpExecContext {
    let mut gated = build_ctx.gated.clone();
    gated.market_row.receive_address = receive_address.to_string();
    CoinOpExecContext::with_resolved_asset(
        gated,
        split_asset_id.to_string(),
        HashSet::new(),
        HashSet::new(),
        #[cfg(test)]
        test_overrides,
    )
}

async fn submit_bootstrap_vault_mixed_split(
    build_ctx: &ResolvedBuildAndPostContext,
    split_asset_id: &str,
    receive_address: &str,
    coin_ids: &[String],
    output_amounts_mojos: Vec<u64>,
    #[cfg(test)] test_overrides: Option<&CoinOpTestOverrides>,
) -> SignerResult<Value> {
    let exec = bootstrap_exec_context(
        build_ctx,
        split_asset_id,
        receive_address,
        #[cfg(test)]
        test_overrides.cloned().unwrap_or_default(),
    );
    let client =
        client_for_signer_on_network(&build_ctx.gated.signer, &build_ctx.gated.operator_network)?;
    let result = exec
        .submit_mixed_split(
            output_amounts_mojos,
            coin_ids,
            0,
            false,
            CatSelection::FetchFromCoinset,
            &client,
        )
        .await?;
    Ok(mixed_split_result_json(&result))
}

pub(super) async fn submit_bootstrap_combine(
    build_ctx: &ResolvedBuildAndPostContext,
    bootstrap_plan: &BootstrapPlan,
    split_asset_id: &str,
    receive_address: &str,
    split_asset_mojo_multiplier: i64,
    #[cfg(test)] test_overrides: Option<&CoinOpTestOverrides>,
) -> SignerResult<Value> {
    let BootstrapFundingSource::CombineFirst(inputs) = &bootstrap_plan.funding else {
        return Err(crate::error::SignerError::InvalidPlanValues);
    };
    let output_amounts =
        bootstrap_combine_vault_outputs(inputs, split_asset_mojo_multiplier.max(1))?;
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
    split_asset_mojo_multiplier: i64,
    #[cfg(test)] test_overrides: Option<&CoinOpTestOverrides>,
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
    use crate::coin_ops::execution::CoinOpTestOverrides;
    use crate::offer::bootstrap::{
        bootstrap_combine_vault_outputs, BaseUnits, BootstrapCombineInputs, BootstrapFundingSource,
        BootstrapPlan,
    };
    use crate::offer::operator::build_and_post::sample_resolved_build_and_post_context;

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
        let overrides = CoinOpTestOverrides::default();
        overrides.enqueue_sample_mixed_split_result();
        let plan = combine_first_plan(BootstrapCombineInputs {
            input_coin_ids: vec!["a".repeat(64), "b".repeat(64)],
            selected_total: BaseUnits::new(105),
            target_amount: BaseUnits::new(100),
            exact_match: false,
            cap_applied: true,
        });
        let build_ctx = sample_resolved_build_and_post_context();
        let result = submit_bootstrap_combine(
            &build_ctx,
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
        let build_ctx = sample_resolved_build_and_post_context();
        let plan = sample_split_plan(&"aa".repeat(64));

        let err = submit_bootstrap_mixed_split(
            &build_ctx,
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
        let build_ctx = sample_resolved_build_and_post_context();
        let plan = sample_split_plan("not-a-valid-coin-id");

        let err = submit_bootstrap_mixed_split(
            &build_ctx,
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
