use serde_json::{json, Value};

use crate::error::SignerError;
use crate::vault::mixed_split::MixedSplitResult;
use crate::vault_coinset_scan::{DustCoin, DustCombineBatch, DustPlan};

/// Stable `stderr_tail` values written into batch execution reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BatchReportReason {
    PriorBatchCombineFailed,
    CombineInputVerifyTimeout,
}

impl BatchReportReason {
    pub(crate) const fn stderr_tail(self) -> &'static str {
        match self {
            Self::PriorBatchCombineFailed => "prior_batch_combine_failed",
            Self::CombineInputVerifyTimeout => "combine input verify timeout",
        }
    }
}

pub(crate) fn batch_stderr_tail(err: &SignerError) -> String {
    match err {
        SignerError::CombineInputVerifyTimeout => BatchReportReason::CombineInputVerifyTimeout
            .stderr_tail()
            .to_string(),
        SignerError::Other(msg) => msg.clone(),
        _ => err.to_string(),
    }
}

/// Full dust plan plus an optional cap on combinable batches for this run.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DustBatchRunSelection<'a> {
    plan: &'a DustPlan,
    max_batches: Option<usize>,
}

impl<'a> DustBatchRunSelection<'a> {
    pub(crate) fn new(plan: &'a DustPlan, max_batches: Option<usize>) -> Self {
        Self { plan, max_batches }
    }

    pub(crate) fn plan(&self) -> &'a DustPlan {
        self.plan
    }

    pub(crate) fn combinable_batches(&self) -> &[DustCombineBatch] {
        let all = self.plan.batches.combinable_batches.as_slice();
        match self.max_batches {
            None => all,
            Some(max) => {
                let end = max.min(all.len());
                &all[..end]
            }
        }
    }

    pub(crate) fn selected_count(&self) -> usize {
        self.combinable_batches().len()
    }

    pub(crate) fn planned_count(&self) -> usize {
        self.plan.batches.combinable_batches.len()
    }
}

fn coin_ids_json(coins: &[DustCoin]) -> Value {
    json!(coins.iter().map(|coin| &coin.coin_id).collect::<Vec<_>>())
}

fn batch_coin_ids_json(batch: &DustCombineBatch) -> Value {
    json!(batch
        .items
        .iter()
        .map(|item| item.dust_coin().coin_id)
        .collect::<Vec<_>>())
}

fn single_coin_status_entry(coin: &DustCoin, status: &str) -> Value {
    json!({
        "coin_ids": coin_ids_json(std::slice::from_ref(coin)),
        "status": status,
    })
}

fn extend_entries_with_non_combinable(entries: &mut Vec<Value>, plan: &DustPlan) {
    for coin in &plan.batches.uncombinable {
        entries.push(single_coin_status_entry(coin, "orphan"));
    }
    for coin in &plan.lineage_excluded {
        entries.push(single_coin_status_entry(coin, "lineage_excluded"));
    }
}

pub fn preview_batches_report(selection: &DustBatchRunSelection<'_>, can_combine: bool) -> Value {
    let plan = selection.plan();
    let mut entries = Vec::new();
    for batch in selection.combinable_batches() {
        entries.push(json!({
            "coin_ids": batch_coin_ids_json(batch),
            "status": "preview",
            "would_combine": can_combine,
        }));
    }
    extend_entries_with_non_combinable(&mut entries, plan);
    json!(entries)
}

pub fn executed_batch_entry(batch: &DustCombineBatch, result: &MixedSplitResult) -> Value {
    json!({
        "coin_ids": batch_coin_ids_json(batch),
        "status": "executed",
        "exit_code": 0,
        "payload": {
            "broadcast_status": result.broadcast_status,
            "selected_coin_ids": result.selected_coin_ids,
            "offered_total": result.offered_total,
            "target_total": result.target_total,
            "change_amount": result.change_amount,
        },
    })
}

pub fn failed_batch_entry(batch: &DustCombineBatch, err: &str) -> Value {
    json!({
        "coin_ids": batch_coin_ids_json(batch),
        "status": "failed",
        "exit_code": 1,
        "stderr_tail": err,
    })
}

pub(crate) fn fail_remaining_batches(
    batch_results: &mut Vec<Value>,
    remaining: &[DustCombineBatch],
    stderr_tail: &str,
) {
    for skipped in remaining {
        batch_results.push(failed_batch_entry(skipped, stderr_tail));
    }
}

pub(crate) fn finalize_plan_batches_report(
    mut batch_results: Vec<Value>,
    plan: &DustPlan,
) -> Value {
    extend_entries_with_non_combinable(&mut batch_results, plan);
    json!(batch_results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault_coinset_scan::{plan_dust_batches, DustBatchPlan, DustCoin};

    use super::super::combine_test_support::proven_dust;

    #[test]
    fn batch_report_reasons_use_stable_stderr_tails() {
        assert_eq!(
            BatchReportReason::PriorBatchCombineFailed.stderr_tail(),
            "prior_batch_combine_failed"
        );
        assert_eq!(
            BatchReportReason::CombineInputVerifyTimeout.stderr_tail(),
            "combine input verify timeout"
        );
    }

    #[test]
    fn batch_stderr_tail_maps_special_cases_and_delegates_display() {
        assert_eq!(
            batch_stderr_tail(&SignerError::CombineInputVerifyTimeout),
            "combine input verify timeout"
        );
        assert_eq!(
            batch_stderr_tail(&SignerError::Other("dust batch total is zero".to_string())),
            "dust batch total is zero"
        );
        assert_eq!(
            batch_stderr_tail(&SignerError::PreselectedCatCoinIdsMismatch),
            SignerError::PreselectedCatCoinIdsMismatch.to_string()
        );
    }

    #[test]
    fn preview_batches_report_uses_unified_schema() {
        let proven: Vec<_> = (0..3)
            .map(|i| proven_dust(&format!("{i:064x}"), 1))
            .collect();
        let batches = plan_dust_batches(&proven, 2);
        let plan = DustPlan {
            scan_dust_count: 3,
            batches,
            lineage_excluded: Vec::new(),
        };
        let selection = DustBatchRunSelection::new(&plan, None);
        let report = preview_batches_report(&selection, true);
        let entries = report.as_array().expect("batch array");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].get("status"), Some(&json!("preview")));
        assert_eq!(entries[0].get("would_combine"), Some(&json!(true)));
        assert_eq!(entries[1].get("status"), Some(&json!("orphan")));
    }

    #[test]
    fn preview_batches_report_includes_lineage_excluded_entries() {
        let plan = DustPlan {
            scan_dust_count: 1,
            batches: DustBatchPlan {
                combinable_batches: Vec::new(),
                uncombinable: Vec::new(),
            },
            lineage_excluded: vec![DustCoin {
                coin_id: "ab".repeat(32),
                amount: 100,
            }],
        };
        let selection = DustBatchRunSelection::new(&plan, None);
        let report = preview_batches_report(&selection, false);
        let entries = report.as_array().expect("batch array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].get("status"), Some(&json!("lineage_excluded")));
    }

    #[test]
    fn preview_batches_report_respects_max_batches() {
        let proven: Vec<_> = (0..4)
            .map(|i| proven_dust(&format!("{i:064x}"), 1))
            .collect();
        let batches = plan_dust_batches(&proven, 2);
        let plan = DustPlan {
            scan_dust_count: 4,
            batches,
            lineage_excluded: Vec::new(),
        };
        let selection = DustBatchRunSelection::new(&plan, Some(1));
        assert_eq!(selection.planned_count(), 2);
        assert_eq!(selection.selected_count(), 1);
        let report = preview_batches_report(&selection, true);
        let entries = report.as_array().expect("batch array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].get("status"), Some(&json!("preview")));
    }
}
