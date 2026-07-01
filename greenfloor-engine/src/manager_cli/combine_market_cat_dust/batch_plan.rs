use serde_json::Value;

use crate::error::SignerResult;
use crate::vault::mixed_split::MixedSplitResult;
use crate::vault_coinset_scan::{DustCombineBatch, DustPlan};

use super::batches::{
    executed_batch_entry, fail_remaining_batches, failed_batch_entry, finalize_plan_batches_report,
    DustBatchRunSelection,
};

pub(crate) struct CombineBatchPlanOutcome {
    pub job_failed: bool,
    pub batches: Value,
}

pub(crate) trait BatchPlanRunner {
    async fn combine_batch(&self, batch: &DustCombineBatch) -> SignerResult<MixedSplitResult>;
    async fn wait_for_batch_spent(&self, batch: &DustCombineBatch) -> SignerResult<()>;
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
                batch_results.push(failed_batch_entry(batch, &err.to_string()));
                fail_remaining_batches(&mut batch_results, remaining, "prior_batch_combine_failed");
                return plan_outcome(true, batch_results, plan);
            }
        }
        if !remaining.is_empty() {
            if let Err(err) = runner.wait_for_batch_spent(batch).await {
                fail_remaining_batches(&mut batch_results, remaining, &err.to_string());
                job_failed = true;
                break;
            }
        }
    }

    plan_outcome(job_failed, batch_results, plan)
}
