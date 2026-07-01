use serde_json::json;

use super::batches::{
    executed_batch_entry, fail_remaining_batches_with_tail, BatchReportReason,
    DustBatchRunSelection,
};
use super::combine_test_support::{
    dust_combine_batch_from_ids, ok_mixed_split_result, sample_combine_batch_plan, RECEIVE_ADDRESS,
};
use super::execute::{run_batch_plan, CombineBatchExecutor};
use crate::coinset::CoinSpentVerifyConfig;
use crate::coinset::CoinsetClient;

const TEST_CAT_ASSET_ID: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const DEAD_COINSET_URL: &str = "http://127.0.0.1:1";

fn test_executor() -> CombineBatchExecutor {
    CombineBatchExecutor::new(
        crate::test_support::signer_config::test_signer_config(DEAD_COINSET_URL),
        RECEIVE_ADDRESS.to_string(),
        TEST_CAT_ASSET_ID.to_string(),
        CoinsetClient::new(DEAD_COINSET_URL.to_string()),
        CoinSpentVerifyConfig::default(),
    )
}

fn assert_batch_entries(entries: &[serde_json::Value], checks: &[(usize, &str, Option<&str>)]) {
    for (index, status, stderr) in checks {
        let entry = &entries[*index];
        assert_eq!(entry.get("status"), Some(&json!(status)));
        if let Some(stderr) = stderr {
            assert_eq!(entry.get("stderr_tail"), Some(&json!(stderr)));
        }
    }
}

#[tokio::test]
async fn run_batch_plan_fails_remaining_when_combine_fails() {
    let plan = sample_combine_batch_plan();
    let selection = DustBatchRunSelection::new(&plan, None);
    let outcome = run_batch_plan(&test_executor(), &selection).await;

    assert!(outcome.job_failed);
    let entries = outcome.batches.as_array().expect("batch array");
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0].get("status"), Some(&json!("failed")));
    assert_batch_entries(
        entries,
        &[
            (1, "failed", Some(BatchReportReason::PriorBatchCombineFailed.stderr_tail())),
            (2, "failed", Some(BatchReportReason::PriorBatchCombineFailed.stderr_tail())),
        ],
    );
}

#[tokio::test]
async fn run_batch_plan_respects_max_batches() {
    let plan = sample_combine_batch_plan();
    let selection = DustBatchRunSelection::new(&plan, Some(1));
    let outcome = run_batch_plan(&test_executor(), &selection).await;

    assert!(outcome.job_failed);
    let entries = outcome.batches.as_array().expect("batch array");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].get("status"), Some(&json!("failed")));
    assert_eq!(entries[1].get("status"), Some(&json!("orphan")));
}

#[test]
fn verify_timeout_skip_uses_stable_stderr_on_remaining_batches() {
    let first = dust_combine_batch_from_ids(&[1]);
    let remaining = [
        dust_combine_batch_from_ids(&[2]),
        dust_combine_batch_from_ids(&[3]),
    ];
    let mut batch_results = vec![executed_batch_entry(&first, &ok_mixed_split_result())];
    fail_remaining_batches_with_tail(
        &mut batch_results,
        &remaining,
        BatchReportReason::CombineInputVerifyTimeout.stderr_tail(),
    );
    assert_eq!(batch_results.len(), 3);
    assert_eq!(batch_results[0].get("status"), Some(&json!("executed")));
    assert_batch_entries(
        &batch_results,
        &[
            (
                1,
                "failed",
                Some(BatchReportReason::CombineInputVerifyTimeout.stderr_tail()),
            ),
            (
                2,
                "failed",
                Some(BatchReportReason::CombineInputVerifyTimeout.stderr_tail()),
            ),
        ],
    );
}
