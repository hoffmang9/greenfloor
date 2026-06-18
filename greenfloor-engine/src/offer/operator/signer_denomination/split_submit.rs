use serde_json::{json, Value};

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
            asset_id: crate::vault::members::hex_to_bytes32(split_asset_id)?,
            output_amounts: bootstrap_plan
                .output_amounts_base_units
                .iter()
                .map(|amount| u64::try_from(*amount).unwrap_or(0))
                .collect(),
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
