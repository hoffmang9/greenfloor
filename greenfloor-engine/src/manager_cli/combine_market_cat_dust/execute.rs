use chia_protocol::Bytes32;
use serde_json::{json, Value};

use crate::coinset::{
    client_for_config, wait_until_coins_spent, CoinSpentVerifyConfig, CoinsetClient,
    MIN_CAT_OUTPUT_MOJOS,
};
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::hex::hex_to_bytes32;
use crate::vault::mixed_split::{
    build_and_optionally_broadcast_vault_cat_mixed_split, MixedSplitRequest, MixedSplitResult,
};
use crate::vault_coinset_scan::{DustBatchPlan, DustCoin};

use super::batches::{append_orphan_entries, executed_batch_entry, failed_batch_entry};

trait BatchDriver {
    async fn run_batch(&self, batch: &[DustCoin]) -> SignerResult<MixedSplitResult>;
    async fn wait_spent(&self, coin_ids: &[Bytes32]) -> SignerResult<()>;
}

struct ProductionBatchDriver<'a> {
    signer_config: SignerConfig,
    receive_address: String,
    cat_asset_id: String,
    client: CoinsetClient,
    verify: CoinSpentVerifyConfig,
    _lifetime: std::marker::PhantomData<&'a ()>,
}

impl ProductionBatchDriver<'_> {
    fn new(
        signer_config: SignerConfig,
        receive_address: String,
        cat_asset_id: String,
        client: CoinsetClient,
        verify: CoinSpentVerifyConfig,
    ) -> Self {
        Self {
            signer_config,
            receive_address,
            cat_asset_id,
            client,
            verify,
            _lifetime: std::marker::PhantomData,
        }
    }
}

impl BatchDriver for ProductionBatchDriver<'_> {
    async fn run_batch(&self, batch: &[DustCoin]) -> SignerResult<MixedSplitResult> {
        run_dust_combine_batch(
            self.signer_config.clone(),
            &self.receive_address,
            &self.cat_asset_id,
            batch,
        )
        .await
    }

    async fn wait_spent(&self, coin_ids: &[Bytes32]) -> SignerResult<()> {
        wait_until_coins_spent(&self.client, coin_ids, self.verify).await
    }
}

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

async fn drive_combine_batch_plan<D: BatchDriver>(
    plan: &DustBatchPlan,
    driver: &D,
) -> (bool, Value) {
    let mut batch_results = Vec::new();
    let mut job_failed = false;
    let batch_count = plan.combinable_batches.len();
    for (index, batch) in plan.combinable_batches.iter().enumerate() {
        match driver.run_batch(batch).await {
            Ok(result) => {
                batch_results.push(executed_batch_entry(batch, &result));
                if index + 1 < batch_count {
                    match batch_coin_ids(batch) {
                        Ok(coin_ids) => {
                            if let Err(err) = driver.wait_spent(&coin_ids).await {
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
    let driver = ProductionBatchDriver::new(
        signer_config.clone(),
        receive_address.to_string(),
        cat_asset_id.to_string(),
        client,
        verify,
    );
    drive_combine_batch_plan(plan, &driver).await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::*;
    use crate::error::SignerError;
    use crate::vault::mixed_split::MixedSplitResult;

    struct MockBatchDriver {
        batch_calls: Arc<AtomicUsize>,
        wait_calls: Arc<AtomicUsize>,
        fail_wait: bool,
        fail_combine_after_first: bool,
    }

    impl BatchDriver for MockBatchDriver {
        async fn run_batch(&self, _batch: &[DustCoin]) -> SignerResult<MixedSplitResult> {
            let attempt = self.batch_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_combine_after_first && attempt > 0 {
                return Err(SignerError::Other("combine failed".to_string()));
            }
            Ok(ok_split_result())
        }

        async fn wait_spent(&self, _coin_ids: &[Bytes32]) -> SignerResult<()> {
            self.wait_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_wait {
                Err(SignerError::CombineInputVerifyTimeout)
            } else {
                Ok(())
            }
        }
    }

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
        let batch_calls = Arc::new(AtomicUsize::new(0));
        let wait_calls = Arc::new(AtomicUsize::new(0));
        let driver = MockBatchDriver {
            batch_calls: Arc::clone(&batch_calls),
            wait_calls: Arc::clone(&wait_calls),
            fail_wait: false,
            fail_combine_after_first: false,
        };

        let (failed, batches) = drive_combine_batch_plan(&plan, &driver).await;

        assert!(!failed);
        assert_eq!(batch_calls.load(Ordering::SeqCst), 3);
        assert_eq!(wait_calls.load(Ordering::SeqCst), 2);
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
        let driver = MockBatchDriver {
            batch_calls: Arc::new(AtomicUsize::new(0)),
            wait_calls: Arc::new(AtomicUsize::new(0)),
            fail_wait: true,
            fail_combine_after_first: false,
        };
        let (failed, batches) = drive_combine_batch_plan(&plan, &driver).await;

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
        let batch_calls = Arc::new(AtomicUsize::new(0));
        let driver = MockBatchDriver {
            batch_calls: Arc::clone(&batch_calls),
            wait_calls: Arc::new(AtomicUsize::new(0)),
            fail_wait: false,
            fail_combine_after_first: true,
        };
        let (failed, batches) = drive_combine_batch_plan(&plan, &driver).await;

        assert!(failed);
        assert_eq!(batch_calls.load(Ordering::SeqCst), 2);
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

    #[test]
    fn all_batches_failed_marks_every_combinable_batch_and_orphans() {
        let plan = sample_plan();
        let (failed, batches) = all_batches_failed(&plan, "client unavailable");
        assert!(failed);
        let entries = batches.as_array().expect("batch array");
        assert_eq!(entries.len(), 4);
        assert!(entries
            .iter()
            .take(3)
            .all(|entry| entry.get("status") == Some(&json!("failed"))));
        assert_eq!(entries[3].get("status"), Some(&json!("orphan")));
    }

    #[tokio::test]
    async fn run_dust_combine_batch_rejects_zero_total_batch() {
        let err = run_dust_combine_batch(
            crate::test_support::signer_config::test_signer_config("http://127.0.0.1:1"),
            "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
            &"f".repeat(64),
            &[DustCoin {
                coin_id: "a".repeat(64),
                amount: 0,
            }],
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("dust batch total is zero"));
    }

    #[tokio::test]
    async fn run_dust_combine_batch_rejects_invalid_coin_id() {
        let err = run_dust_combine_batch(
            crate::test_support::signer_config::test_signer_config("http://127.0.0.1:1"),
            "xch1a0t57qn6uhe7tzjlxlhwy2qgmuxvvft8gnfzmg5detg0q9f3yc3s2apz0h",
            &"f".repeat(64),
            &[DustCoin {
                coin_id: "not-valid-hex".to_string(),
                amount: 100,
            }],
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("invalid hex"));
    }
}
