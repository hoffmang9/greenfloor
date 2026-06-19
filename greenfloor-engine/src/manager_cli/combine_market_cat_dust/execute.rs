use std::future::Future;
use std::pin::Pin;

use chia_protocol::Bytes32;
use serde_json::{json, Value};

use crate::coinset::{
    client_for_config, wait_until_coins_spent, CoinSpentVerifyConfig, MIN_CAT_OUTPUT_MOJOS,
};
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

fn batch_coin_ids(batch: &[DustCoin]) -> SignerResult<Vec<Bytes32>> {
    batch
        .iter()
        .map(|coin| hex_to_bytes32(&coin.coin_id))
        .collect()
}

fn fail_remaining_batches(
    batch_results: &mut Vec<Value>,
    remaining: &[Vec<DustCoin>],
    reason: &str,
) {
    for skipped in remaining {
        batch_results.push(failed_batch_entry(skipped, reason));
    }
}

fn all_batches_failed(plan: &DustBatchPlan, reason: &str) -> (bool, Value) {
    let mut batch_results = Vec::new();
    for batch in &plan.combinable_batches {
        batch_results.push(failed_batch_entry(batch, reason));
    }
    let mut batches_json = json!(batch_results);
    append_orphan_entries(&mut batches_json, &plan.uncombinable);
    (true, batches_json)
}

type BatchRunnerFuture<'a> =
    Pin<Box<dyn Future<Output = SignerResult<MixedSplitResult>> + Send + 'a>>;
type WaitSpentFuture<'a> = Pin<Box<dyn Future<Output = SignerResult<()>> + Send + 'a>>;

pub(crate) async fn execute_combine_batches_with_hooks(
    plan: &DustBatchPlan,
    mut run_batch: impl FnMut(&[DustCoin]) -> BatchRunnerFuture<'_>,
    mut wait_spent: impl FnMut(&[Bytes32]) -> WaitSpentFuture<'_>,
) -> (bool, Value) {
    let mut batch_results = Vec::new();
    let mut job_failed = false;
    let batch_count = plan.combinable_batches.len();
    for (index, batch) in plan.combinable_batches.iter().enumerate() {
        match run_batch(batch).await {
            Ok(result) => {
                batch_results.push(executed_batch_entry(batch, &result));
                if index + 1 < batch_count {
                    match batch_coin_ids(batch) {
                        Ok(coin_ids) => {
                            if let Err(err) = wait_spent(&coin_ids).await {
                                job_failed = true;
                                fail_remaining_batches(
                                    &mut batch_results,
                                    &plan.combinable_batches[index + 1..],
                                    &err.to_string(),
                                );
                                break;
                            }
                        }
                        Err(err) => {
                            job_failed = true;
                            fail_remaining_batches(
                                &mut batch_results,
                                &plan.combinable_batches[index + 1..],
                                &err.to_string(),
                            );
                            break;
                        }
                    }
                }
            }
            Err(err) => {
                job_failed = true;
                batch_results.push(failed_batch_entry(batch, &err.to_string()));
                fail_remaining_batches(
                    &mut batch_results,
                    &plan.combinable_batches[index + 1..],
                    "prior_batch_combine_failed",
                );
                break;
            }
        }
    }
    let mut batches_json = json!(batch_results);
    append_orphan_entries(&mut batches_json, &plan.uncombinable);
    (job_failed, batches_json)
}

pub async fn execute_combine_batches(
    signer_config: &SignerConfig,
    receive_address: &str,
    cat_asset_id: &str,
    plan: &DustBatchPlan,
    verify: CoinSpentVerifyConfig,
) -> (bool, Value) {
    let client = match client_for_config(signer_config) {
        Ok(client) => client,
        Err(err) => return all_batches_failed(plan, &err.to_string()),
    };
    let signer_config = signer_config.clone();
    let receive_address = receive_address.to_string();
    let cat_asset_id = cat_asset_id.to_string();
    execute_combine_batches_with_hooks(
        plan,
        move |batch| {
            let signer_config = signer_config.clone();
            let receive_address = receive_address.clone();
            let cat_asset_id = cat_asset_id.clone();
            let batch = batch.to_vec();
            Box::pin(async move {
                run_dust_combine_batch(signer_config, &receive_address, &cat_asset_id, &batch).await
            })
        },
        move |coin_ids| {
            let client = client.clone();
            let coin_ids = coin_ids.to_vec();
            Box::pin(async move { wait_until_coins_spent(&client, &coin_ids, verify).await })
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SignerError;
    use crate::vault::mixed_split::MixedSplitResult;

    fn dust_batch(ids: &[u8]) -> Vec<DustCoin> {
        ids.iter()
            .map(|id| DustCoin {
                coin_id: format!("{id:064x}"),
                amount: 100,
            })
            .collect()
    }

    fn ok_split_result() -> MixedSplitResult {
        MixedSplitResult {
            spend_bundle_hex: String::new(),
            broadcast_status: Some("submitted".to_string()),
            selected_coin_ids: vec!["aa".repeat(64)],
            offered_total: 200,
            target_total: 200,
            change_amount: 0,
        }
    }

    fn sample_plan() -> DustBatchPlan {
        DustBatchPlan {
            combinable_batches: vec![dust_batch(&[1]), dust_batch(&[2]), dust_batch(&[3])],
            uncombinable: vec![DustCoin {
                coin_id: "f".repeat(64),
                amount: 1,
            }],
        }
    }

    #[tokio::test]
    async fn execute_waits_between_batches_and_runs_all_when_verify_succeeds() {
        let plan = sample_plan();
        let batch_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let wait_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let (failed, batches) = execute_combine_batches_with_hooks(
            &plan,
            {
                let batch_calls = std::sync::Arc::clone(&batch_calls);
                move |_batch| {
                    batch_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Box::pin(async { Ok(ok_split_result()) })
                }
            },
            {
                let wait_calls = std::sync::Arc::clone(&wait_calls);
                move |_coin_ids| {
                    wait_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Box::pin(async { Ok(()) })
                }
            },
        )
        .await;

        assert!(!failed);
        assert_eq!(batch_calls.load(std::sync::atomic::Ordering::SeqCst), 3);
        assert_eq!(wait_calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        let entries = batches.as_array().expect("batch array");
        assert_eq!(entries.len(), 4);
        assert!(entries
            .iter()
            .take(3)
            .all(|entry| entry.get("status") == Some(&json!("executed"))));
        assert_eq!(entries[3].get("status"), Some(&json!("orphan")));
    }

    #[tokio::test]
    async fn execute_skips_remaining_batches_when_verify_times_out() {
        let plan = sample_plan();
        let (failed, batches) = execute_combine_batches_with_hooks(
            &plan,
            |_batch| Box::pin(async { Ok(ok_split_result()) }),
            |_| Box::pin(async { Err(SignerError::CombineInputVerifyTimeout) }),
        )
        .await;

        assert!(failed);
        let entries = batches.as_array().expect("batch array");
        assert_eq!(entries[0].get("status"), Some(&json!("executed")));
        assert_eq!(entries[1].get("status"), Some(&json!("failed")));
        assert_eq!(entries[2].get("status"), Some(&json!("failed")));
        assert_eq!(
            entries[1].get("stderr_tail"),
            Some(&json!("combine input verify timeout"))
        );
    }

    #[tokio::test]
    async fn execute_skips_remaining_batches_when_combine_fails() {
        let plan = sample_plan();
        let batch_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (failed, batches) = execute_combine_batches_with_hooks(
            &plan,
            {
                let batch_calls = std::sync::Arc::clone(&batch_calls);
                move |_batch| {
                    let attempt = batch_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Box::pin(async move {
                        if attempt == 0 {
                            Ok(ok_split_result())
                        } else {
                            Err(SignerError::Other("combine failed".to_string()))
                        }
                    })
                }
            },
            |_| Box::pin(async { Ok(()) }),
        )
        .await;

        assert!(failed);
        let entries = batches.as_array().expect("batch array");
        assert_eq!(entries[0].get("status"), Some(&json!("executed")));
        assert_eq!(entries[1].get("status"), Some(&json!("failed")));
        assert_eq!(
            entries[1].get("stderr_tail"),
            Some(&json!("combine failed"))
        );
        assert_eq!(entries[2].get("status"), Some(&json!("failed")));
        assert_eq!(
            entries[2].get("stderr_tail"),
            Some(&json!("prior_batch_combine_failed"))
        );
    }
}
