mod batches;
mod coinset_context;
mod execute;
mod jobs;
mod report;
#[cfg(test)]
mod report_test;
#[cfg(test)]
mod test_support;

use coinset_context::{
    load_execution_signer, resolve_combine_coinset_context, CombineCoinsetContext,
};
use jobs::{build_enabled_cat_jobs, CatDustJob};
use report::{
    finalize_job_report, list_failed_job_report, signer_blocked_job_report, CombineRunMode,
};
use serde_json::{json, Value};

use crate::coinset::CoinSpentVerifyConfig;
use crate::config::{
    load_markets_config_with_overlay, parse_program_config, read_program_yaml,
    ManagerProgramConfig, MarketsConfig, SignerConfig,
};
use crate::error::{SignerError, SignerResult};
use crate::manager_cli::context::ManagerContext;
use crate::vault_coinset_scan::{
    build_cat_dust_scan_request, cache_resolved_launcher_id, resolve_launcher_id,
    CatDustScanParams, ResolveLauncherIdParams, ScanResult, ScanState,
};

pub use report::CombineExecutionFlags;

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
    pub verify: CoinSpentVerifyConfig,
    pub execution: CombineExecutionFlags,
}

struct ProcessJobContext<'a> {
    mgr: &'a ManagerContext,
    program: &'a ManagerProgramConfig,
    coinset: &'a CombineCoinsetContext,
    launcher_id: &'a str,
    max_nonce: u32,
    dust_threshold_mojos: u64,
    max_input_coins: usize,
    run_mode: &'a CombineRunMode<'a>,
    job: &'a CatDustJob,
}

fn emit_command_error(
    mgr: &ManagerContext,
    reason: &str,
    detail: impl std::fmt::Display,
) -> SignerResult<i32> {
    mgr.emit_json(&json!({
        "status": "error",
        "reason": reason,
        "detail": detail.to_string(),
    }))?;
    Ok(1)
}

fn signer_load_error_reason(err: &SignerError) -> &'static str {
    if matches!(err, SignerError::SignerPathNotConfigured) {
        "signer_not_configured"
    } else {
        "signer_load_failed"
    }
}

async fn run_vault_scan_for_job(
    mgr: &ManagerContext,
    coinset: &CombineCoinsetContext,
    launcher_id: &str,
    max_nonce: u32,
    cat_asset_id: &str,
) -> SignerResult<ScanResult> {
    let request = build_cat_dust_scan_request(&CatDustScanParams {
        network: &coinset.network,
        coinset_base_url: Some(coinset.direct_base_url_for_scan()),
        launcher_id,
        max_nonce,
        cat_asset_id,
        cats_config: &mgr.cats_config,
        markets_config: &mgr.markets_config,
        testnet_markets_config: mgr.testnet_markets_path(),
    });
    ScanState::run(request).await
}

async fn process_job(ctx: ProcessJobContext<'_>) -> SignerResult<Value> {
    let readiness = report::vault_signer_ready(ctx.program, &ctx.job.signer_key_id);
    if matches!(ctx.run_mode, CombineRunMode::Execute { .. })
        && readiness.note == Some("unknown_signer_key_id")
    {
        return Ok(signer_blocked_job_report(
            ctx.job,
            readiness.note.expect("unknown_signer_key_id"),
        ));
    }

    let scan_result = match run_vault_scan_for_job(
        ctx.mgr,
        ctx.coinset,
        ctx.launcher_id,
        ctx.max_nonce,
        &ctx.job.cat_asset_id,
    )
    .await
    {
        Ok(result) => result,
        Err(err) => return Ok(list_failed_job_report(ctx.job, &err.to_string())),
    };

    finalize_job_report(
        ctx.job,
        scan_result,
        ctx.dust_threshold_mojos,
        ctx.max_input_coins,
        ctx.run_mode,
        readiness,
    )
    .await
}

fn load_program_and_markets(
    mgr: &ManagerContext,
) -> SignerResult<(serde_json::Value, ManagerProgramConfig, MarketsConfig)> {
    let raw = read_program_yaml(&mgr.program_config)?;
    let program = parse_program_config(&raw)?;
    let markets =
        load_markets_config_with_overlay(&mgr.markets_config, mgr.testnet_markets_path())?;
    Ok((raw, program, markets))
}

pub async fn run_combine_market_cat_dust(
    request: CombineMarketCatDustRequest<'_>,
) -> SignerResult<i32> {
    let flags = request.execution;
    let mgr = request.mgr;

    if !mgr.cats_config.is_file() {
        return emit_command_error(
            mgr,
            "cats_config_missing",
            mgr.cats_config.display().to_string(),
        );
    }

    let (raw, program, markets) = match load_program_and_markets(mgr) {
        Ok(loaded) => loaded,
        Err(err) => return emit_command_error(mgr, "config_validate_failed", err),
    };
    let coinset_ctx = resolve_combine_coinset_context(
        request.network,
        request.coinset_base_url,
        &program.network,
        &CombineCoinsetContext::program_default_msp_base_url(&raw),
    );

    let execution_signer: Option<SignerConfig> = if flags.is_preview() {
        None
    } else {
        match load_execution_signer(&raw, program.clone(), &coinset_ctx) {
            Ok(signer) => Some(signer),
            Err(err) => return emit_command_error(mgr, signer_load_error_reason(&err), err),
        }
    };

    let jobs = match build_enabled_cat_jobs(&markets, &mgr.cats_config, request.cat_asset_id) {
        Ok(jobs) => jobs,
        Err(err) => return emit_command_error(mgr, "config", err),
    };

    if jobs.is_empty() {
        mgr.emit_json(&json!({
            "status": "ok",
            "message": "no_enabled_cat_markets",
            "network": coinset_ctx.network,
            "jobs": [],
        }))?;
        return Ok(0);
    }

    let resolved_launcher = match resolve_launcher_id(&ResolveLauncherIdParams {
        launcher_id: request.launcher_id,
        launcher_id_file: request.launcher_id_file,
        program_config: Some(&mgr.program_config),
    }) {
        Ok(resolved) => resolved,
        Err(err) => return emit_command_error(mgr, "launcher_id_resolution_failed", err),
    };
    if let Err(err) = cache_resolved_launcher_id(
        request.launcher_id_file,
        resolved_launcher.source,
        &resolved_launcher.launcher_id,
    ) {
        return emit_command_error(mgr, "launcher_id_cache_failed", err);
    }

    let run_mode = match execution_signer.as_ref() {
        Some(signer) => CombineRunMode::Execute {
            signer,
            verify: request.verify,
        },
        None => CombineRunMode::Preview,
    };

    let mut exit_code = 0;
    let mut job_reports = Vec::new();

    for job in jobs {
        let job_report = process_job(ProcessJobContext {
            mgr,
            program: &program,
            coinset: &coinset_ctx,
            launcher_id: &resolved_launcher.launcher_id,
            max_nonce: request.max_nonce,
            dust_threshold_mojos: request.dust_threshold_mojos,
            max_input_coins: request.max_input_coins,
            run_mode: &run_mode,
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
        "network": coinset_ctx.network,
        "dust_threshold_mojos": request.dust_threshold_mojos,
        "dry_run": flags.dry_run,
        "list_only": flags.list_only,
        "jobs": job_reports,
    }))?;
    Ok(exit_code)
}
