use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::json;

use super::batch_plan::{run_batch_plan, BatchPlanRunner};
use super::batches::DustBatchRunSelection;
use super::combine_test_support::{
    dust_combine_batch_from_ids, ok_mixed_split_result, sample_combine_batch_plan, RECEIVE_ADDRESS,
};
use super::execute::CombineBatchExecutor;
use crate::coinset::test_support::{
    coin_record_by_name_request_json, mock_get_coin_record_by_name_body,
    mock_unspent_coin_record_by_name_body,
};
use crate::coinset::{CoinSpentVerifyConfig, CoinsetClient};
use crate::error::{SignerError, SignerResult};
use crate::vault::mixed_split::MixedSplitResult;
use crate::vault_coinset_scan::{DustCombineBatch, DustPlan, ProvenDustCoin};

const TEST_CAT_ASSET_ID: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

fn test_combine_batch_executor(
    coinset_url: &str,
    cat_asset_id: &str,
    verify: CoinSpentVerifyConfig,
) -> CombineBatchExecutor {
    CombineBatchExecutor::new(
        crate::test_support::signer_config::test_signer_config(coinset_url),
        RECEIVE_ADDRESS.to_string(),
        cat_asset_id.to_string(),
        CoinsetClient::new(coinset_url.to_string()),
        verify,
    )
}

struct MockBatchPlanRunner {
    batch_calls: Arc<AtomicUsize>,
    wait_calls: Arc<AtomicUsize>,
    fail_wait: bool,
    fail_combine_after_first: bool,
}

impl MockBatchPlanRunner {
    fn new(fail_wait: bool, fail_combine_after_first: bool) -> Self {
        Self {
            batch_calls: Arc::new(AtomicUsize::new(0)),
            wait_calls: Arc::new(AtomicUsize::new(0)),
            fail_wait,
            fail_combine_after_first,
        }
    }
}

impl BatchPlanRunner for MockBatchPlanRunner {
    async fn combine_batch(&self, _batch: &DustCombineBatch) -> SignerResult<MixedSplitResult> {
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

/// `(entry_index, status, optional stderr_tail)`
type EntryCheck = (usize, &'static str, Option<&'static str>);

struct PlanRunExpect {
    fail_wait: bool,
    fail_combine_after_first: bool,
    max_batches: Option<usize>,
    job_failed: bool,
    batch_calls: usize,
    wait_calls: usize,
    entry_count: usize,
    entries: &'static [EntryCheck],
}

async fn assert_plan_run(plan: &DustPlan, expect: PlanRunExpect) {
    let runner = MockBatchPlanRunner::new(expect.fail_wait, expect.fail_combine_after_first);
    let batch_calls = Arc::clone(&runner.batch_calls);
    let wait_calls = Arc::clone(&runner.wait_calls);
    let selection = DustBatchRunSelection::new(plan, expect.max_batches);
    let outcome = run_batch_plan(&runner, &selection).await;

    assert_eq!(outcome.job_failed, expect.job_failed);
    assert_eq!(batch_calls.load(Ordering::SeqCst), expect.batch_calls);
    assert_eq!(wait_calls.load(Ordering::SeqCst), expect.wait_calls);
    let entries = outcome.batches.as_array().expect("batch array");
    assert_eq!(entries.len(), expect.entry_count);
    for (index, status, stderr) in expect.entries {
        let entry = &entries[*index];
        assert_eq!(entry.get("status"), Some(&json!(status)));
        if let Some(stderr) = stderr {
            assert_eq!(entry.get("stderr_tail"), Some(&json!(stderr)));
        }
    }
}

#[tokio::test]
async fn execute_waits_between_batches_and_runs_all_when_verify_succeeds() {
    assert_plan_run(
        &sample_combine_batch_plan(),
        PlanRunExpect {
            fail_wait: false,
            fail_combine_after_first: false,
            max_batches: None,
            job_failed: false,
            batch_calls: 3,
            wait_calls: 2,
            entry_count: 4,
            entries: &[
                (0, "executed", None),
                (1, "executed", None),
                (2, "executed", None),
                (3, "orphan", None),
            ],
        },
    )
    .await;
}

#[tokio::test]
async fn execute_skips_remaining_batches_when_verify_times_out() {
    assert_plan_run(
        &sample_combine_batch_plan(),
        PlanRunExpect {
            fail_wait: true,
            fail_combine_after_first: false,
            max_batches: None,
            job_failed: true,
            batch_calls: 1,
            wait_calls: 1,
            entry_count: 4,
            entries: &[
                (0, "executed", None),
                (1, "failed", Some("combine input verify timeout")),
            ],
        },
    )
    .await;
}

#[tokio::test]
async fn execute_skips_remaining_batches_when_combine_fails() {
    assert_plan_run(
        &sample_combine_batch_plan(),
        PlanRunExpect {
            fail_wait: false,
            fail_combine_after_first: true,
            max_batches: None,
            job_failed: true,
            batch_calls: 2,
            wait_calls: 1,
            entry_count: 4,
            entries: &[
                (0, "executed", None),
                (1, "failed", Some("combine failed")),
                (2, "failed", Some("prior_batch_combine_failed")),
            ],
        },
    )
    .await;
}

#[tokio::test]
async fn execute_respects_max_batches() {
    assert_plan_run(
        &sample_combine_batch_plan(),
        PlanRunExpect {
            fail_wait: false,
            fail_combine_after_first: false,
            max_batches: Some(1),
            job_failed: false,
            batch_calls: 1,
            wait_calls: 0,
            entry_count: 2,
            entries: &[(0, "executed", None), (1, "orphan", None)],
        },
    )
    .await;
}

#[tokio::test]
async fn combine_batch_executor_rejects_zero_total_batch() {
    let mut cat = crate::coinset::test_support::cat_with_amount(0);
    cat.coin = chia_protocol::Coin::new(
        crate::hex::hex_to_bytes32(&"a".repeat(64)).expect("coin id"),
        cat.coin.puzzle_hash,
        0,
    );
    let err = BatchPlanRunner::combine_batch(
        &test_combine_batch_executor(
            "http://127.0.0.1:1",
            TEST_CAT_ASSET_ID,
            CoinSpentVerifyConfig::default(),
        ),
        &DustCombineBatch {
            items: vec![ProvenDustCoin::from_cat(cat)],
        },
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("dust batch total is zero"));
}

#[tokio::test]
async fn combine_batch_executor_rejects_invalid_cat_asset_id() {
    let err = BatchPlanRunner::combine_batch(
        &test_combine_batch_executor(
            "http://127.0.0.1:1",
            "not-valid-hex",
            CoinSpentVerifyConfig::default(),
        ),
        &dust_combine_batch_from_ids(&[1]),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("invalid hex"));
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

    BatchPlanRunner::wait_for_batch_spent(
        &test_combine_batch_executor(
            &server.url(),
            TEST_CAT_ASSET_ID,
            CoinSpentVerifyConfig {
                timeout_seconds: 5,
                poll_seconds: 1,
            },
        ),
        &batch,
    )
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

    let err = BatchPlanRunner::wait_for_batch_spent(
        &test_combine_batch_executor(
            &server.url(),
            TEST_CAT_ASSET_ID,
            CoinSpentVerifyConfig {
                timeout_seconds: 1,
                poll_seconds: 1,
            },
        ),
        &batch,
    )
    .await
    .expect_err("verify timeout");
    assert!(matches!(err, SignerError::CombineInputVerifyTimeout));
}
