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
use crate::vault_coinset_scan::{DustCombineBatch, DustPlan};

use super::batches::{
    batch_stderr_tail, executed_batch_entry, fail_remaining_batches, failed_batch_entry,
    finalize_plan_batches_report, BatchReportReason, DustBatchRunSelection,
};

pub(crate) struct CombineBatchPlanOutcome {
    pub job_failed: bool,
    pub batches: Value,
}

/// Test seam for [`run_batch_plan`]; production uses [`CombineBatchExecutor`] only.
#[doc(hidden)]
pub(crate) trait BatchPlanRunner {
    async fn combine_batch(&self, batch: &DustCombineBatch) -> SignerResult<MixedSplitResult>;
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
}

impl BatchPlanRunner for CombineBatchExecutor {
    async fn combine_batch(&self, batch: &DustCombineBatch) -> SignerResult<MixedSplitResult> {
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

    async fn wait_for_batch_spent(&self, batch: &DustCombineBatch) -> SignerResult<()> {
        let coin_ids = batch.coin_ids()?;
        wait_until_coins_spent(&self.client, &coin_ids, self.verify).await
    }
}

fn plan_outcome(
    job_failed: bool,
    batch_results: Vec<Value>,
    plan: &DustPlan,
) -> CombineBatchPlanOutcome {
    CombineBatchPlanOutcome {
        job_failed,
        batches: finalize_plan_batches_report(batch_results, plan),
    }
}

#[allow(clippy::large_futures)]
pub(crate) async fn run_batch_plan<R: BatchPlanRunner>(
    runner: &R,
    selection: &DustBatchRunSelection<'_>,
) -> CombineBatchPlanOutcome {
    let mut batch_results = Vec::new();
    let mut job_failed = false;
    let batches_to_run = selection.combinable_batches();
    let plan = selection.plan();

    for (index, batch) in batches_to_run.iter().enumerate() {
        let remaining = &batches_to_run[index + 1..];
        match runner.combine_batch(batch).await {
            Ok(result) => batch_results.push(executed_batch_entry(batch, &result)),
            Err(err) => {
                batch_results.push(failed_batch_entry(batch, &batch_stderr_tail(&err)));
                fail_remaining_batches(
                    &mut batch_results,
                    remaining,
                    BatchReportReason::PriorBatchCombineFailed.stderr_tail(),
                );
                return plan_outcome(true, batch_results, plan);
            }
        }
        if !remaining.is_empty() {
            if let Err(err) = runner.wait_for_batch_spent(batch).await {
                fail_remaining_batches(&mut batch_results, remaining, &batch_stderr_tail(&err));
                job_failed = true;
                break;
            }
        }
    }

    plan_outcome(job_failed, batch_results, plan)
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
    run_batch_plan(
        &CombineBatchExecutor::new(
            signer_config.clone(),
            receive_address.to_string(),
            cat_asset_id.to_string(),
            client.clone(),
            verify,
        ),
        selection,
    )
    .await
}
