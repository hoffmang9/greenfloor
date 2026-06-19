mod batches;
mod execute;
mod jobs;
mod report;
#[cfg(test)]
mod report_test;
#[cfg(test)]
mod test_support;

use jobs::{build_enabled_cat_jobs, CatDustJob};
use report::{
    finalize_job_report, list_failed_job_report, signer_blocked_job_report, CombineExecutionFlags,
    CombineRunMode,
};
use serde_json::{json, Value};

use crate::coinset::normalize_coinset_network;
use crate::coinset::CoinSpentVerifyConfig;
use crate::config::{
    load_markets_config_with_overlay, parse_program_config, program_bundle_gated_from_parsed,
    read_program_yaml, ManagerProgramConfig, MarketsConfig,
};
use crate::error::{SignerError, SignerResult};
use crate::manager_cli::context::ManagerContext;
use crate::vault_coinset_scan::{
    build_cat_dust_scan_request, cache_resolved_launcher_id, resolve_launcher_id,
    CatDustScanParams, ResolveLauncherIdParams, ScanResult, ScanState,
};

pub use report::CombineExecutionFlags as CombineExecution;

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
    network: &'a str,
    coinset_base_url: Option<&'a str>,
    launcher_id: &'a str,
    max_nonce: u32,
    dust_threshold_mojos: u64,
    max_input_coins: usize,
    run_mode: &'a CombineRunMode<'a>,
    job: &'a CatDustJob,
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
        ctx.network,
        ctx.coinset_base_url,
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

fn resolved_network(request_network: Option<&str>, program_network: &str) -> String {
    match request_network
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => normalize_coinset_network(value).to_string(),
        None => normalize_coinset_network(program_network).to_string(),
    }
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
        mgr.emit_json(&json!({
            "status": "error",
            "reason": "cats_config_missing",
            "detail": mgr.cats_config.display().to_string(),
        }))?;
        return Ok(1);
    }

    let (raw, program, markets) = match load_program_and_markets(mgr) {
        Ok(loaded) => loaded,
        Err(err) => {
            mgr.emit_json(&json!({
                "status": "error",
                "reason": "config_validate_failed",
                "detail": err.to_string(),
            }))?;
            return Ok(1);
        }
    };
    let network = resolved_network(request.network, &program.network);

    let combine_signer = if flags.is_preview() {
        None
    } else {
        match program_bundle_gated_from_parsed(program.clone(), &raw) {
            Err(SignerError::SignerPathNotConfigured) => {
                mgr.emit_json(&json!({
                    "status": "error",
                    "reason": "signer_not_configured",
                    "detail": SignerError::SignerPathNotConfigured.to_string(),
                }))?;
                return Ok(1);
            }
            Err(err) => return Err(err),
            Ok(bundle) => Some(bundle.signer),
        }
    };

    let jobs = match build_enabled_cat_jobs(&markets, &mgr.cats_config, request.cat_asset_id) {
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

    let resolved_launcher = match resolve_launcher_id(&ResolveLauncherIdParams {
        launcher_id: request.launcher_id,
        launcher_id_file: request.launcher_id_file,
        program_config: Some(&mgr.program_config),
    }) {
        Ok(resolved) => resolved,
        Err(err) => {
            mgr.emit_json(&json!({
                "status": "error",
                "reason": "launcher_id_resolution_failed",
                "detail": err.to_string(),
            }))?;
            return Ok(1);
        }
    };
    if let Err(err) = cache_resolved_launcher_id(
        request.launcher_id_file,
        resolved_launcher.source,
        &resolved_launcher.launcher_id,
    ) {
        mgr.emit_json(&json!({
            "status": "error",
            "reason": "launcher_id_cache_failed",
            "detail": err.to_string(),
        }))?;
        return Ok(1);
    }

    let run_mode = match combine_signer.as_ref() {
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
            network: &network,
            coinset_base_url: request.coinset_base_url,
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
        "network": network,
        "dust_threshold_mojos": request.dust_threshold_mojos,
        "dry_run": flags.dry_run,
        "list_only": flags.list_only,
        "jobs": job_reports,
    }))?;
    Ok(exit_code)
}
