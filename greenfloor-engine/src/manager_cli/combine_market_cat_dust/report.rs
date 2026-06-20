use serde_json::{json, Value};

use super::batches::preview_batches_report;
use super::jobs::CatDustJob;
use crate::coinset::CoinSpentVerifyConfig;
use crate::config::{ManagerProgramConfig, SignerConfig};
use crate::error::SignerResult;
use crate::vault_coinset_scan::{DustBatchPlan, ScanResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombineExecutionFlags {
    pub dry_run: bool,
    pub list_only: bool,
}

impl CombineExecutionFlags {
    #[must_use]
    pub const fn from_flags(list_only: bool, dry_run: bool) -> Self {
        Self { dry_run, list_only }
    }

    #[must_use]
    pub const fn is_preview(self) -> bool {
        self.list_only || self.dry_run
    }
}

#[derive(Debug)]
pub(crate) enum CombineRunMode<'a> {
    Preview,
    Execute {
        signer: &'a SignerConfig,
        verify: CoinSpentVerifyConfig,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VaultSignerReadiness {
    pub can_combine: bool,
    pub note: Option<&'static str>,
}

/// Per-job readiness: registry key id must exist; KMS/vault path is gated globally for combine.
pub(crate) fn vault_signer_ready(
    program: &ManagerProgramConfig,
    signer_key_id: &str,
) -> VaultSignerReadiness {
    if !program.signer_key_registry.contains_key(signer_key_id) {
        return VaultSignerReadiness {
            can_combine: false,
            note: Some("unknown_signer_key_id"),
        };
    }
    if !program.signer_offer_path_configured() {
        return VaultSignerReadiness {
            can_combine: false,
            note: Some("signer_not_configured"),
        };
    }
    VaultSignerReadiness {
        can_combine: true,
        note: None,
    }
}

pub(crate) fn job_report_base(job: &CatDustJob) -> Value {
    json!({
        "cat_asset_id": job.cat_asset_id,
        "signer_key_id": job.signer_key_id,
        "market_ids": job.market_ids,
        "receive_address": job.receive_address,
    })
}

pub(crate) fn attach_list_summary(job_report: &mut Value, scan: &ScanResult) {
    job_report["list"] = json!({
        "exit_code": 0,
        "summary": {
            "count": scan.count,
            "max_nonce_scanned": scan.max_nonce_scanned,
            "launcher_id": scan.launcher_id,
        },
    });
}

pub(crate) fn attach_dust_plan_fields(
    job_report: &mut Value,
    dust_coin_count: usize,
    plan: &DustBatchPlan,
) {
    job_report["dust_coin_count"] = json!(dust_coin_count);
    job_report["combine_batches_planned"] = json!(plan.combinable_batches.len());
    job_report["uncombinable_dust_count"] = json!(plan.uncombinable.len());
}

pub(crate) fn list_failed_job_report(job: &CatDustJob, err: &str) -> Value {
    let mut report = job_report_base(job);
    report["status"] = json!("error");
    report["reason"] = json!("list_failed");
    report["list"] = json!({
        "exit_code": 1,
        "stderr_tail": err,
    });
    report
}

pub(crate) fn signer_blocked_job_report(job: &CatDustJob, reason: &str) -> Value {
    let mut report = job_report_base(job);
    report["status"] = json!("error");
    report["reason"] = json!(reason);
    report
}

pub(crate) fn preview_job_report(
    job: &CatDustJob,
    scan: &ScanResult,
    plan: &DustBatchPlan,
    dust_coin_count: usize,
    readiness: VaultSignerReadiness,
) -> Value {
    let mut report = job_report_base(job);
    attach_list_summary(&mut report, scan);
    attach_dust_plan_fields(&mut report, dust_coin_count, plan);
    report["status"] = json!("ok");
    report["signer_config_ok"] = json!(readiness.can_combine);
    if let Some(note) = readiness.note {
        report["signer_config_note"] = json!(note);
    }
    report["batches"] = preview_batches_report(plan, readiness.can_combine);
    report
}

pub(crate) fn combine_job_report(
    job: &CatDustJob,
    scan: &ScanResult,
    plan: &DustBatchPlan,
    dust_coin_count: usize,
    batches: Value,
    job_failed: bool,
) -> Value {
    let mut report = job_report_base(job);
    attach_list_summary(&mut report, scan);
    attach_dust_plan_fields(&mut report, dust_coin_count, plan);
    report["status"] = json!(if job_failed { "error" } else { "ok" });
    report["batches"] = batches;
    report
}

pub(crate) fn plan_dust_for_scan(
    scan: &ScanResult,
    dust_threshold_mojos: u64,
    max_input_coins: usize,
) -> (usize, DustBatchPlan) {
    let dust_coins =
        crate::vault_coinset_scan::dust_coins_from_scan(&scan.coins, dust_threshold_mojos);
    let dust_coin_count = dust_coins.len();
    let plan = crate::vault_coinset_scan::plan_dust_batches(&dust_coins, max_input_coins);
    (dust_coin_count, plan)
}

pub(crate) async fn finalize_job_report(
    job: &CatDustJob,
    scan: ScanResult,
    dust_threshold_mojos: u64,
    max_input_coins: usize,
    run_mode: &CombineRunMode<'_>,
    readiness: VaultSignerReadiness,
) -> SignerResult<Value> {
    let (dust_coin_count, plan) = plan_dust_for_scan(&scan, dust_threshold_mojos, max_input_coins);
    Ok(match run_mode {
        CombineRunMode::Preview => {
            preview_job_report(job, &scan, &plan, dust_coin_count, readiness)
        }
        CombineRunMode::Execute { signer, verify } => {
            let (job_failed, batches) = Box::pin(super::execute::execute_combine_batches(
                signer,
                &job.receive_address,
                &job.cat_asset_id,
                &plan,
                *verify,
            ))
            .await;
            combine_job_report(job, &scan, &plan, dust_coin_count, batches, job_failed)
        }
    })
}
