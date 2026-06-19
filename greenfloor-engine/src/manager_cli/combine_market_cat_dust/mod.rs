mod batches;
mod jobs;

use std::path::Path;

use batches::{
    append_orphan_entries, executed_batch_entry, failed_batch_entry, preview_batches_report,
};
use chia_protocol::Bytes32;
use jobs::{build_enabled_cat_jobs, CatDustJob};
use serde_json::{json, Value};

use crate::coinset::normalize_coinset_network;
use crate::coinset::MIN_CAT_OUTPUT_MOJOS;
use crate::config::{load_program_bundle_gated, load_program_config, SignerConfig};
use crate::error::{SignerError, SignerResult};
use crate::manager_cli::context::ManagerContext;
use crate::manager_cli::setup;
use crate::vault::members::hex_to_bytes32;
use crate::vault::mixed_split::{
    build_and_optionally_broadcast_vault_cat_mixed_split, MixedSplitRequest, MixedSplitResult,
};
use crate::vault_coinset_scan::{
    build_cat_dust_scan_request, cache_resolved_launcher_id, dust_coins_from_scan,
    plan_dust_batches, resolve_launcher_id, CatDustScanParams, DustCoin, ResolveLauncherIdParams,
    ScanResult, ScanState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombineExecution {
    ListOnly,
    DryRun,
    Combine,
}

impl CombineExecution {
    #[must_use]
    pub const fn from_flags(list_only: bool, dry_run: bool) -> Self {
        if list_only {
            Self::ListOnly
        } else if dry_run {
            Self::DryRun
        } else {
            Self::Combine
        }
    }
}

#[derive(Debug, Clone)]
pub struct CombineMarketCatDustRequest<'a> {
    pub mgr: &'a ManagerContext,
    pub network: Option<&'a str>,
    pub coinset_base_url: Option<&'a str>,
    pub launcher_id: Option<&'a str>,
    pub launcher_id_file: Option<&'a str>,
    pub dust_threshold_mojos: u64,
    pub max_input_coins: usize,
    pub max_nonce: u32,
    pub cat_asset_id: Option<&'a str>,
    pub execution: CombineExecution,
}

struct SignerKeyReadiness {
    can_combine: bool,
    note: Option<&'static str>,
}

fn signer_key_ready(program_path: &Path, signer_key_id: &str) -> SignerKeyReadiness {
    let Ok(program) = load_program_config(program_path) else {
        return SignerKeyReadiness {
            can_combine: false,
            note: Some("program_config_unreadable"),
        };
    };
    let Some(entry) = program.signer_key_registry.get(signer_key_id) else {
        return SignerKeyReadiness {
            can_combine: false,
            note: Some("unknown_signer_key_id"),
        };
    };
    if entry
        .keyring_yaml_path
        .as_deref()
        .map(str::trim)
        .is_none_or(str::is_empty)
    {
        return SignerKeyReadiness {
            can_combine: false,
            note: Some("missing_keyring_yaml_path"),
        };
    }
    SignerKeyReadiness {
        can_combine: true,
        note: None,
    }
}

async fn run_vault_scan_for_job(
    mgr: &ManagerContext,
    network: &str,
    coinset_base_url: Option<&str>,
    launcher_id: &str,
    max_nonce: u32,
    cat_asset_id: &str,
) -> SignerResult<ScanResult> {
    let request = build_cat_dust_scan_request(&CatDustScanParams {
        network,
        coinset_base_url,
        launcher_id,
        max_nonce,
        cat_asset_id,
        cats_config: &mgr.cats_config,
        markets_config: &mgr.markets_config,
        testnet_markets_config: mgr.testnet_markets_path(),
    });
    ScanState::run(request).await
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

async fn execute_combine_batches(
    signer_config: &SignerConfig,
    receive_address: &str,
    cat_asset_id: &str,
    batches: &[Vec<DustCoin>],
) -> (bool, Value) {
    let mut batch_results = Vec::new();
    let mut job_failed = false;
    for batch in batches {
        match run_dust_combine_batch(signer_config.clone(), receive_address, cat_asset_id, batch)
            .await
        {
            Ok(result) => batch_results.push(executed_batch_entry(batch, &result)),
            Err(err) => {
                job_failed = true;
                batch_results.push(failed_batch_entry(batch, &err.to_string()));
            }
        }
    }
    (job_failed, json!(batch_results))
}

struct ProcessJobContext<'a> {
    mgr: &'a ManagerContext,
    network: &'a str,
    coinset_base_url: Option<&'a str>,
    launcher_id: &'a str,
    max_nonce: u32,
    dust_threshold_mojos: u64,
    max_input_coins: usize,
    execution: CombineExecution,
    signer_config: Option<&'a SignerConfig>,
    job: &'a CatDustJob,
}

async fn process_job(ctx: ProcessJobContext<'_>) -> SignerResult<Value> {
    let mut job_report = json!({
        "cat_asset_id": ctx.job.cat_asset_id,
        "signer_key_id": ctx.job.signer_key_id,
        "market_ids": ctx.job.market_ids,
        "receive_address": ctx.job.receive_address,
    });

    let signer_ready = signer_key_ready(&ctx.mgr.program_config, &ctx.job.signer_key_id);
    if ctx.execution == CombineExecution::Combine && !signer_ready.can_combine {
        job_report["status"] = json!("error");
        job_report["reason"] = json!(signer_ready.note);
        return Ok(job_report);
    }

    let scan_result = match run_vault_scan_for_job(
        ctx.mgr,
        ctx.network,
        ctx.coinset_base_url,
        ctx.launcher_id,
        ctx.max_nonce,
        &ctx.job.cat_asset_id,
    )
    .await
    {
        Ok(result) => result,
        Err(err) => {
            job_report["status"] = json!("error");
            job_report["reason"] = json!("list_failed");
            job_report["list"] = json!({
                "exit_code": 1,
                "stderr_tail": err.to_string(),
            });
            return Ok(job_report);
        }
    };

    job_report["list"] = json!({
        "exit_code": 0,
        "summary": {
            "count": scan_result.count,
            "max_nonce_scanned": scan_result.max_nonce_scanned,
            "launcher_id": scan_result.launcher_id,
        },
    });

    let dust_coins = dust_coins_from_scan(&scan_result.coins, ctx.dust_threshold_mojos);
    let batch_plan = plan_dust_batches(&dust_coins, ctx.max_input_coins);
    job_report["dust_coin_count"] = json!(dust_coins.len());
    job_report["combine_batches_planned"] = json!(batch_plan.combinable_batches.len());
    job_report["uncombinable_dust_count"] = json!(batch_plan.uncombinable.len());

    if ctx.execution != CombineExecution::Combine {
        job_report["status"] = json!("ok");
        job_report["signer_config_ok"] = json!(signer_ready.can_combine);
        if let Some(note) = signer_ready.note {
            job_report["signer_config_note"] = json!(note);
        }
        job_report["batches"] = preview_batches_report(&batch_plan, signer_ready.can_combine);
        return Ok(job_report);
    }

    let signer_config = ctx
        .signer_config
        .expect("combine execution loads signer config");
    let (job_failed, mut batches_json) = execute_combine_batches(
        signer_config,
        &ctx.job.receive_address,
        &ctx.job.cat_asset_id,
        &batch_plan.combinable_batches,
    )
    .await;
    append_orphan_entries(&mut batches_json, &batch_plan.uncombinable);
    job_report["status"] = json!(if job_failed { "error" } else { "ok" });
    job_report["batches"] = batches_json;
    Ok(job_report)
}

fn resolved_network(request_network: Option<&str>, program_network: &str) -> String {
    match request_network
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => normalize_coinset_network(value).to_string(),
        None => normalize_coinset_network(program_network).to_string(),
    }
}

pub async fn run_combine_market_cat_dust(
    request: CombineMarketCatDustRequest<'_>,
) -> SignerResult<i32> {
    let execution = request.execution;
    let mgr = request.mgr;

    if !mgr.cats_config.is_file() {
        mgr.emit_json(&json!({
            "status": "error",
            "reason": "cats_config_missing",
            "detail": mgr.cats_config.display().to_string(),
        }))?;
        return Ok(1);
    }

    if let Err(err) = setup::validate_config(mgr, false) {
        mgr.emit_json(&json!({
            "status": "error",
            "reason": "config_validate_failed",
            "detail": err.to_string(),
        }))?;
        return Ok(1);
    }

    let bundle = load_program_bundle_gated(&mgr.program_config)?;
    let network = resolved_network(request.network, &bundle.program.network);

    let jobs = match build_enabled_cat_jobs(
        &mgr.markets_config,
        mgr.testnet_markets_path(),
        &mgr.cats_config,
        request.cat_asset_id,
    ) {
        Ok(jobs) => jobs,
        Err(err) => {
            mgr.emit_json(&json!({
                "status": "error",
                "reason": "config",
                "detail": err.to_string(),
            }))?;
            return Ok(1);
        }
    };

    if jobs.is_empty() {
        mgr.emit_json(&json!({
            "status": "ok",
            "message": "no_enabled_cat_markets",
            "network": network,
            "jobs": [],
        }))?;
        return Ok(0);
    }

    let resolved_launcher = resolve_launcher_id(&ResolveLauncherIdParams {
        launcher_id: request.launcher_id,
        launcher_id_file: request.launcher_id_file,
        program_config: Some(&mgr.program_config),
    })?;
    cache_resolved_launcher_id(
        request.launcher_id_file,
        resolved_launcher.source,
        &resolved_launcher.launcher_id,
    )?;

    let signer_config = (execution == CombineExecution::Combine).then_some(&bundle.signer);
    let mut exit_code = 0;
    let mut job_reports = Vec::new();

    for job in jobs {
        let job_report = process_job(ProcessJobContext {
            mgr,
            network: &network,
            coinset_base_url: request.coinset_base_url,
            launcher_id: &resolved_launcher.launcher_id,
            max_nonce: request.max_nonce,
            dust_threshold_mojos: request.dust_threshold_mojos,
            max_input_coins: request.max_input_coins,
            execution,
            signer_config,
            job: &job,
        })
        .await?;
        if job_report.get("status").and_then(Value::as_str) == Some("error") {
            exit_code = 1;
        }
        job_reports.push(job_report);
    }

    mgr.emit_json(&json!({
        "status": if exit_code == 0 { "ok" } else { "error" },
        "network": network,
        "dust_threshold_mojos": request.dust_threshold_mojos,
        "dry_run": execution == CombineExecution::DryRun,
        "list_only": execution == CombineExecution::ListOnly,
        "jobs": job_reports,
    }))?;
    Ok(exit_code)
}
