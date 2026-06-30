use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use chia_protocol::Bytes32;
use serde_json::json;

use super::batches::{all_batches_failed, DustBatchRunSelection};
use super::combine_test_support::{
    dust_combine_batch_from_ids, ok_mixed_split_result, sample_combine_batch_plan, RECEIVE_ADDRESS,
};
use super::execute::{drive_combine_batch_plan, BatchDriver, CombineBatchExecutor};
use crate::coinset::CoinsetClient;
use crate::error::SignerError;
use crate::vault::mixed_split::MixedSplitResult;
use crate::vault_coinset_scan::{DustCombineBatch, ProvenDustCoin};

struct MockBatchDriver {
    batch_calls: Arc<AtomicUsize>,
    wait_calls: Arc<AtomicUsize>,
    fail_wait: bool,
    fail_combine_after_first: bool,
}

impl BatchDriver for MockBatchDriver {
    async fn run_batch(
        &self,
        _batch: &DustCombineBatch,
    ) -> crate::error::SignerResult<MixedSplitResult> {
        let attempt = self.batch_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_combine_after_first && attempt > 0 {
            return Err(SignerError::Other("combine failed".to_string()));
        }
        Ok(ok_mixed_split_result())
    }

    async fn wait_spent(&self, _coin_ids: &[Bytes32]) -> crate::error::SignerResult<()> {
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
    let driver = MockBatchDriver {
        batch_calls: Arc::clone(&batch_calls),
        wait_calls: Arc::clone(&wait_calls),
        fail_wait: false,
        fail_combine_after_first: false,
    };

    let selection = DustBatchRunSelection::new(&plan, None);
    let (failed, batches) = drive_combine_batch_plan(&selection, &driver).await;

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
    let plan = sample_combine_batch_plan();
    let driver = MockBatchDriver {
        batch_calls: Arc::new(AtomicUsize::new(0)),
        wait_calls: Arc::new(AtomicUsize::new(0)),
        fail_wait: true,
        fail_combine_after_first: false,
    };
    let selection = DustBatchRunSelection::new(&plan, None);
    let (failed, batches) = drive_combine_batch_plan(&selection, &driver).await;

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
    let plan = sample_combine_batch_plan();
    let batch_calls = Arc::new(AtomicUsize::new(0));
    let driver = MockBatchDriver {
        batch_calls: Arc::clone(&batch_calls),
        wait_calls: Arc::new(AtomicUsize::new(0)),
        fail_wait: false,
        fail_combine_after_first: true,
    };
    let selection = DustBatchRunSelection::new(&plan, None);
    let (failed, batches) = drive_combine_batch_plan(&selection, &driver).await;

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

#[tokio::test]
async fn execute_respects_max_batches() {
    let plan = sample_combine_batch_plan();
    let batch_calls = Arc::new(AtomicUsize::new(0));
    let wait_calls = Arc::new(AtomicUsize::new(0));
    let driver = MockBatchDriver {
        batch_calls: Arc::clone(&batch_calls),
        wait_calls: Arc::clone(&wait_calls),
        fail_wait: false,
        fail_combine_after_first: false,
    };

    let selection = DustBatchRunSelection::new(&plan, Some(1));
    let (failed, batches) = drive_combine_batch_plan(&selection, &driver).await;

    assert!(!failed);
    assert_eq!(batch_calls.load(Ordering::SeqCst), 1);
    assert_eq!(wait_calls.load(Ordering::SeqCst), 0);
    let entries = batches.as_array().expect("batch array");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].get("status"), Some(&json!("executed")));
    assert_eq!(entries[1].get("status"), Some(&json!("orphan")));
}

#[test]
fn all_batches_failed_marks_every_combinable_batch_and_orphans() {
    let plan = sample_combine_batch_plan();
    let selection = DustBatchRunSelection::new(&plan, None);
    let batches = all_batches_failed(&selection, "client unavailable");
    let entries = batches.as_array().expect("batch array");
    assert_eq!(entries.len(), 4);
    assert!(entries
        .iter()
        .take(3)
        .all(|entry| entry.get("status") == Some(&json!("failed"))));
    assert_eq!(entries[3].get("status"), Some(&json!("orphan")));
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
        crate::coinset::CoinSpentVerifyConfig::default(),
    );
    let err = executor
        .run_batch(&DustCombineBatch {
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
        crate::coinset::CoinSpentVerifyConfig::default(),
    );
    let err = executor
        .run_batch(&dust_combine_batch_from_ids(&[1]))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("invalid hex"));
}
