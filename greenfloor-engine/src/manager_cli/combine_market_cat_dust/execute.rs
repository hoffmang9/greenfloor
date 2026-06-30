use serde_json::Value;

use crate::coinset::{
    wait_until_coins_spent, CoinSpentVerifyConfig, CoinsetClient, MIN_CAT_OUTPUT_MOJOS,
};
use crate::config::SignerConfig;
use crate::error::{SignerError, SignerResult};
use crate::hex::hex_to_bytes32;
use crate::vault::mixed_split::{
    build_and_optionally_broadcast_vault_cat_mixed_split_with_preselected_cats, MixedSplitRequest,
    MixedSplitResult,
};
use crate::vault_coinset_scan::DustCombineBatch;

use super::batches::{
    executed_batch_entry, fail_remaining_batches, failed_batch_entry, finalize_plan_batches_report,
    DustBatchRunSelection,
};

pub(crate) struct CombineBatchPlanOutcome {
    pub job_failed: bool,
    pub batches: Value,
}

trait BatchPlanRunner {
    async fn run_batch(&self, batch: &DustCombineBatch) -> SignerResult<MixedSplitResult>;
    async fn wait_for_batch_spent(&self, batch: &DustCombineBatch) -> SignerResult<()>;
}

pub(crate) struct CombineBatchExecutor {
    signer_config: SignerConfig,
    receive_address: String,
    cat_asset_id: String,
    client: CoinsetClient,
    verify: CoinSpentVerifyConfig,
}

impl CombineBatchExecutor {
    pub(crate) fn new(
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
        }
    }

    pub(crate) async fn combine_batch(
        &self,
        batch: &DustCombineBatch,
    ) -> SignerResult<MixedSplitResult> {
        let total = batch.total_amount();
        if total == 0 {
            return Err(SignerError::Other("dust batch total is zero".to_string()));
        }
        let coin_ids = batch.coin_ids()?;
        let request = MixedSplitRequest {
            receive_address: self.receive_address.clone(),
            asset_id: hex_to_bytes32(&self.cat_asset_id)?,
            output_amounts: vec![total],
            coin_ids,
            allow_sub_cat_output: total < MIN_CAT_OUTPUT_MOJOS,
            fee_mojos: 0,
        };
        build_and_optionally_broadcast_vault_cat_mixed_split_with_preselected_cats(
            self.signer_config.clone(),
            request,
            batch.cats(),
            true,
            &self.client,
        )
        .await
    }

    #[allow(clippy::large_futures)]
    pub async fn run(&self, selection: &DustBatchRunSelection<'_>) -> CombineBatchPlanOutcome {
        run_batch_plan(self, selection).await
    }
}

impl BatchPlanRunner for CombineBatchExecutor {
    async fn run_batch(&self, batch: &DustCombineBatch) -> SignerResult<MixedSplitResult> {
        self.combine_batch(batch).await
    }

    async fn wait_for_batch_spent(&self, batch: &DustCombineBatch) -> SignerResult<()> {
        let coin_ids = batch.coin_ids()?;
        wait_until_coins_spent(&self.client, &coin_ids, self.verify).await
    }
}

#[allow(clippy::large_futures)]
async fn run_batch_plan<R: BatchPlanRunner>(
    runner: &R,
    selection: &DustBatchRunSelection<'_>,
) -> CombineBatchPlanOutcome {
    let mut batch_results = Vec::new();
    let mut job_failed = false;
    let batches_to_run = selection.combinable_batches();
    let plan = selection.plan();

    for (index, batch) in batches_to_run.iter().enumerate() {
        let remaining = &batches_to_run[index + 1..];
        match runner.run_batch(batch).await {
            Ok(result) => batch_results.push(executed_batch_entry(batch, &result)),
            Err(err) => {
                batch_results.push(failed_batch_entry(batch, &err.to_string()));
                fail_remaining_batches(&mut batch_results, remaining, "prior_batch_combine_failed");
                return CombineBatchPlanOutcome {
                    job_failed: true,
                    batches: finalize_plan_batches_report(batch_results, plan),
                };
            }
        }
        if remaining.is_empty() {
            continue;
        }
        if let Err(err) = runner.wait_for_batch_spent(batch).await {
            fail_remaining_batches(&mut batch_results, remaining, &err.to_string());
            job_failed = true;
            break;
        }
    }

    CombineBatchPlanOutcome {
        job_failed,
        batches: finalize_plan_batches_report(batch_results, plan),
    }
}

#[allow(clippy::large_futures)]
pub async fn execute_combine_batches(
    signer_config: &SignerConfig,
    client: &CoinsetClient,
    receive_address: &str,
    cat_asset_id: &str,
    selection: &DustBatchRunSelection<'_>,
    verify: CoinSpentVerifyConfig,
) -> CombineBatchPlanOutcome {
    CombineBatchExecutor::new(
        signer_config.clone(),
        receive_address.to_string(),
        cat_asset_id.to_string(),
        client.clone(),
        verify,
    )
    .run(selection)
    .await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use serde_json::json;

    use super::super::combine_test_support::{
        dust_combine_batch_from_ids, ok_mixed_split_result, sample_combine_batch_plan,
        RECEIVE_ADDRESS,
    };
    use super::*;
    use crate::coinset::test_support::{
        coin_record_by_name_request_json, mock_get_coin_record_by_name_body,
        mock_unspent_coin_record_by_name_body,
    };
    use crate::error::SignerError;
    use crate::vault_coinset_scan::{DustCombineBatch, ProvenDustCoin};

    struct MockBatchPlanRunner {
        batch_calls: Arc<AtomicUsize>,
        wait_calls: Arc<AtomicUsize>,
        fail_wait: bool,
        fail_combine_after_first: bool,
    }

    impl BatchPlanRunner for MockBatchPlanRunner {
        async fn run_batch(&self, _batch: &DustCombineBatch) -> SignerResult<MixedSplitResult> {
            let attempt = self.batch_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_combine_after_first && attempt > 0 {
                return Err(SignerError::Other("combine failed".to_string()));
            }
            Ok(ok_mixed_split_result())
        }

        async fn wait_for_batch_spent(&self, _batch: &DustCombineBatch) -> SignerResult<()> {
            self.wait_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_wait {
                Err(SignerError::CombineInputVerifyTimeout)
            } else {
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn execute_waits_between_batches_and_runs_all_when_verify_succeeds() {
        let plan = sample_combine_batch_plan();
        let batch_calls = Arc::new(AtomicUsize::new(0));
        let wait_calls = Arc::new(AtomicUsize::new(0));
        let runner = MockBatchPlanRunner {
            batch_calls: Arc::clone(&batch_calls),
            wait_calls: Arc::clone(&wait_calls),
            fail_wait: false,
            fail_combine_after_first: false,
        };

        let selection = DustBatchRunSelection::new(&plan, None);
        let outcome = run_batch_plan(&runner, &selection).await;

        assert!(!outcome.job_failed);
        assert_eq!(batch_calls.load(Ordering::SeqCst), 3);
        assert_eq!(wait_calls.load(Ordering::SeqCst), 2);
        let entries = outcome.batches.as_array().expect("batch array");
        assert_eq!(entries.len(), 4);
        assert!(entries
            .iter()
            .take(3)
            .all(|entry| entry.get("status") == Some(&json!("executed"))));
        assert_eq!(entries[3].get("status"), Some(&json!("orphan")));
    }

    #[tokio::test]
    async fn execute_skips_remaining_batches_when_verify_times_out() {
        let plan = sample_combine_batch_plan();
        let runner = MockBatchPlanRunner {
            batch_calls: Arc::new(AtomicUsize::new(0)),
            wait_calls: Arc::new(AtomicUsize::new(0)),
            fail_wait: true,
            fail_combine_after_first: false,
        };
        let selection = DustBatchRunSelection::new(&plan, None);
        let outcome = run_batch_plan(&runner, &selection).await;

        assert!(outcome.job_failed);
        let entries = outcome.batches.as_array().expect("batch array");
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
        let plan = sample_combine_batch_plan();
        let batch_calls = Arc::new(AtomicUsize::new(0));
        let runner = MockBatchPlanRunner {
            batch_calls: Arc::clone(&batch_calls),
            wait_calls: Arc::new(AtomicUsize::new(0)),
            fail_wait: false,
            fail_combine_after_first: true,
        };
        let selection = DustBatchRunSelection::new(&plan, None);
        let outcome = run_batch_plan(&runner, &selection).await;

        assert!(outcome.job_failed);
        assert_eq!(batch_calls.load(Ordering::SeqCst), 2);
        let entries = outcome.batches.as_array().expect("batch array");
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

    #[tokio::test]
    async fn execute_respects_max_batches() {
        let plan = sample_combine_batch_plan();
        let batch_calls = Arc::new(AtomicUsize::new(0));
        let wait_calls = Arc::new(AtomicUsize::new(0));
        let runner = MockBatchPlanRunner {
            batch_calls: Arc::clone(&batch_calls),
            wait_calls: Arc::clone(&wait_calls),
            fail_wait: false,
            fail_combine_after_first: false,
        };

        let selection = DustBatchRunSelection::new(&plan, Some(1));
        let outcome = run_batch_plan(&runner, &selection).await;

        assert!(!outcome.job_failed);
        assert_eq!(batch_calls.load(Ordering::SeqCst), 1);
        assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
        let entries = outcome.batches.as_array().expect("batch array");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].get("status"), Some(&json!("executed")));
        assert_eq!(entries[1].get("status"), Some(&json!("orphan")));
    }

    #[tokio::test]
    async fn combine_batch_executor_rejects_zero_total_batch() {
        let mut cat = crate::coinset::test_support::cat_with_amount(0);
        cat.coin = chia_protocol::Coin::new(
            crate::hex::hex_to_bytes32(&"a".repeat(64)).expect("coin id"),
            cat.coin.puzzle_hash,
            0,
        );
        let client = CoinsetClient::new("http://127.0.0.1:1".to_string());
        let executor = CombineBatchExecutor::new(
            crate::test_support::signer_config::test_signer_config("http://127.0.0.1:1"),
            RECEIVE_ADDRESS.to_string(),
            "f".repeat(64),
            client,
            CoinSpentVerifyConfig::default(),
        );
        let err = executor
            .combine_batch(&DustCombineBatch {
                items: vec![ProvenDustCoin::from_cat(cat)],
            })
            .await
            .unwrap_err();
        assert!(err.to_string().contains("dust batch total is zero"));
    }

    #[tokio::test]
    async fn combine_batch_executor_rejects_invalid_cat_asset_id() {
        let client = CoinsetClient::new("http://127.0.0.1:1".to_string());
        let executor = CombineBatchExecutor::new(
            crate::test_support::signer_config::test_signer_config("http://127.0.0.1:1"),
            RECEIVE_ADDRESS.to_string(),
            "not-valid-hex".to_string(),
            client,
            CoinSpentVerifyConfig::default(),
        );
        let err = executor
            .combine_batch(&dust_combine_batch_from_ids(&[1]))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("invalid hex"));
    }

    #[tokio::test]
    async fn executor_run_marks_batch_failed_when_combine_fails() {
        let plan = sample_combine_batch_plan();
        let selection = DustBatchRunSelection::new(&plan, Some(1));
        let executor = CombineBatchExecutor::new(
            crate::test_support::signer_config::test_signer_config("http://127.0.0.1:1"),
            RECEIVE_ADDRESS.to_string(),
            "f".repeat(64),
            CoinsetClient::new("http://127.0.0.1:1".to_string()),
            CoinSpentVerifyConfig::default(),
        );
        let outcome = executor.run(&selection).await;
        assert!(outcome.job_failed);
        let entries = outcome.batches.as_array().expect("batch array");
        assert_eq!(entries[0].get("status"), Some(&json!("failed")));
    }

    #[tokio::test]
    async fn combine_batch_executor_waits_until_inputs_are_spent() {
        let batch = dust_combine_batch_from_ids(&[1, 2]);
        let mut server = mockito::Server::new_async().await;
        for item in &batch.items {
            let coin = item.cat().coin;
            server
                .mock("POST", "/get_coin_record_by_name")
                .match_body(mockito::Matcher::PartialJson(
                    coin_record_by_name_request_json(coin.coin_id()),
                ))
                .with_body(mock_get_coin_record_by_name_body(&coin, 100))
                .create_async()
                .await;
        }

        let executor = CombineBatchExecutor::new(
            crate::test_support::signer_config::test_signer_config(&server.url()),
            RECEIVE_ADDRESS.to_string(),
            "f".repeat(64),
            CoinsetClient::new(server.url()),
            CoinSpentVerifyConfig {
                timeout_seconds: 5,
                poll_seconds: 1,
            },
        );

        BatchPlanRunner::wait_for_batch_spent(&executor, &batch)
            .await
            .expect("inputs spent");
    }

    #[tokio::test]
    async fn combine_batch_executor_verify_times_out_when_inputs_stay_unspent() {
        let batch = dust_combine_batch_from_ids(&[3]);
        let coin = batch.items[0].cat().coin;
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/get_coin_record_by_name")
            .match_body(mockito::Matcher::PartialJson(
                coin_record_by_name_request_json(coin.coin_id()),
            ))
            .with_body(mock_unspent_coin_record_by_name_body(&coin))
            .create_async()
            .await;

        let executor = CombineBatchExecutor::new(
            crate::test_support::signer_config::test_signer_config(&server.url()),
            RECEIVE_ADDRESS.to_string(),
            "f".repeat(64),
            CoinsetClient::new(server.url()),
            CoinSpentVerifyConfig {
                timeout_seconds: 1,
                poll_seconds: 1,
            },
        );

        let err = BatchPlanRunner::wait_for_batch_spent(&executor, &batch)
            .await
            .expect_err("verify timeout");
        assert!(matches!(err, SignerError::CombineInputVerifyTimeout));
    }
}
