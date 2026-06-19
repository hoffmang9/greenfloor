use chia_protocol::Bytes32;
use serde_json::{json, Value};

use crate::coinset::MIN_CAT_OUTPUT_MOJOS;
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::vault::members::hex_to_bytes32;
use crate::vault::mixed_split::{
    build_and_optionally_broadcast_vault_cat_mixed_split, MixedSplitRequest, MixedSplitResult,
};
use crate::vault_coinset_scan::{DustBatchPlan, DustCoin};

use super::batches::{append_orphan_entries, executed_batch_entry, failed_batch_entry};

async fn run_dust_combine_batch(
    signer_config: SignerConfig,
    receive_address: &str,
    cat_asset_id: &str,
    batch: &[DustCoin],
) -> SignerResult<MixedSplitResult> {
    let total: u64 = batch.iter().map(|coin| coin.amount).sum();
    if total == 0 {
        return Err(SignerError::Other("dust batch total is zero".to_string()));
    }
    let coin_ids = batch
        .iter()
        .map(|coin| hex_to_bytes32(&coin.coin_id))
        .collect::<SignerResult<Vec<Bytes32>>>()?;
    let request = MixedSplitRequest {
        receive_address: receive_address.to_string(),
        asset_id: hex_to_bytes32(cat_asset_id)?,
        output_amounts: vec![total],
        coin_ids,
        allow_sub_cat_output: total < MIN_CAT_OUTPUT_MOJOS,
        fee_mojos: 0,
    };
    build_and_optionally_broadcast_vault_cat_mixed_split(signer_config, request, true).await
}

pub async fn execute_combine_batches(
    signer_config: &SignerConfig,
    receive_address: &str,
    cat_asset_id: &str,
    plan: &DustBatchPlan,
) -> (bool, Value) {
    let mut batch_results = Vec::new();
    let mut job_failed = false;
    for batch in &plan.combinable_batches {
        match run_dust_combine_batch(signer_config.clone(), receive_address, cat_asset_id, batch)
            .await
        {
            Ok(result) => batch_results.push(executed_batch_entry(batch, &result)),
            Err(err) => {
                job_failed = true;
                batch_results.push(failed_batch_entry(batch, &err.to_string()));
            }
        }
    }
    let mut batches_json = json!(batch_results);
    append_orphan_entries(&mut batches_json, &plan.uncombinable);
    (job_failed, batches_json)
}
