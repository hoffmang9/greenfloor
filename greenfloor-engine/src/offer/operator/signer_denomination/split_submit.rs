use serde_json::{json, Value};

use crate::coin_ops::coin_op_non_negative_u64;
use crate::config::SignerConfig;
use crate::error::SignerResult;
use crate::offer::bootstrap::BootstrapPlan;
use crate::vault::{build_and_optionally_broadcast_vault_cat_mixed_split, MixedSplitRequest};

pub(super) async fn submit_bootstrap_mixed_split(
    signer_config: &SignerConfig,
    bootstrap_plan: &BootstrapPlan,
    split_asset_id: &str,
    receive_address: &str,
) -> SignerResult<Value> {
    let result = build_and_optionally_broadcast_vault_cat_mixed_split(
        signer_config.clone(),
        MixedSplitRequest {
            receive_address: receive_address.to_string(),
            asset_id: crate::hex::hex_to_bytes32(split_asset_id)?,
            output_amounts: bootstrap_plan
                .output_amounts_base_units
                .iter()
                .map(|amount| {
                    coin_op_non_negative_u64(*amount, "bootstrap.output_amount_base_units")
                })
                .collect::<SignerResult<Vec<_>>>()?,
            coin_ids: crate::coinset::parse_coin_ids(std::slice::from_ref(
                &bootstrap_plan.source_coin_id,
            ))?,
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

#[cfg(test)]
mod tests {
    use super::submit_bootstrap_mixed_split;
    use crate::offer::bootstrap::BootstrapPlan;
    use crate::test_support::signer_config::test_signer_config;

    #[tokio::test]
    async fn submit_bootstrap_mixed_split_rejects_invalid_asset_hex() {
        let signer = test_signer_config("https://example.test");
        let plan = BootstrapPlan {
            source_coin_id: "aa".repeat(64),
            source_amount: 1_000,
            output_amounts_base_units: vec![100],
            total_output_amount: 100,
            change_amount: 900,
            deficits: Vec::new(),
        };

        let err = submit_bootstrap_mixed_split(
            &signer,
            &plan,
            "not-a-valid-asset-id",
            "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
        )
        .await
        .expect_err("invalid asset hex");

        assert!(err.to_string().contains("hex"));
    }

    #[tokio::test]
    async fn submit_bootstrap_mixed_split_rejects_invalid_source_coin_id() {
        let signer = test_signer_config("https://example.test");
        let plan = BootstrapPlan {
            source_coin_id: "not-a-valid-coin-id".to_string(),
            source_amount: 1_000,
            output_amounts_base_units: vec![100],
            total_output_amount: 100,
            change_amount: 900,
            deficits: Vec::new(),
        };

        let err = submit_bootstrap_mixed_split(
            &signer,
            &plan,
            &"aa".repeat(64),
            "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
        )
        .await
        .expect_err("invalid coin id");

        assert!(err.to_string().contains("hex"));
    }
}
