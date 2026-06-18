mod context;
mod create;
mod iteration;
mod publish;
mod types;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::adapters::{DexieClient, SplashClient};
use crate::error::{SignerError, SignerResult};
use crate::storage::OfferPostPersistRecord;

use context::resolve_build_and_post_context;
use iteration::run_post_iteration;
use publish::persist_post_records_if_enabled;
use types::{PostIterationOutcome, build_and_post_exit_code};

#[derive(Debug, Clone, Deserialize)]
pub struct BuildAndPostOfferRequest {
    pub program_path: PathBuf,
    pub markets_path: PathBuf,
    pub testnet_markets_path: Option<PathBuf>,
    pub network: String,
    pub market_id: Option<String>,
    pub pair: Option<String>,
    pub size_base_units: u64,
    pub repeat: u32,
    pub publish_venue: Option<String>,
    pub dexie_base_url: Option<String>,
    pub splash_base_url: Option<String>,
    pub drop_only: bool,
    pub claim_rewards: bool,
    pub dry_run: bool,
    pub compact_json: bool,
    pub persist_results: bool,
    /// When set, overrides ``pricing.side`` for bootstrap and offer construction (daemon buy/sell actions).
    pub action_side: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BuildAndPostOfferResponse {
    pub exit_code: i32,
    pub payload: Value,
    pub output: String,
}

pub fn format_build_and_post_output(payload: &Value, compact_json: bool) -> String {
    if compact_json {
        serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string())
    } else {
        serde_json::to_string_pretty(payload).unwrap_or_else(|_| "{}".to_string())
    }
}

pub async fn build_and_post_offer(
    request: BuildAndPostOfferRequest,
) -> SignerResult<BuildAndPostOfferResponse> {
    if request.size_base_units == 0 {
        return Err(SignerError::Other(
            "size_base_units must be positive".to_string(),
        ));
    }
    if request.repeat == 0 {
        return Err(SignerError::Other("repeat must be positive".to_string()));
    }

    let ctx = resolve_build_and_post_context(&request).await?;

    let mut post_results = Vec::new();
    let mut built_offers_preview = Vec::new();
    let mut bootstrap_actions = Vec::new();
    let mut publish_failures = 0u32;
    let mut persist_records: Vec<OfferPostPersistRecord> = Vec::new();

    let dexie = if !request.dry_run && ctx.publish_venue == "dexie" {
        Some(DexieClient::new(ctx.dexie_base_url.clone()))
    } else {
        None
    };
    let splash = if !request.dry_run && ctx.publish_venue == "splash" {
        Some(SplashClient::new(ctx.splash_base_url.clone()))
    } else {
        None
    };

    for _ in 0..request.repeat {
        let (bootstrap_action, iteration) =
            run_post_iteration(&request, &ctx, dexie.as_ref(), splash.as_ref()).await?;
        bootstrap_actions.push(bootstrap_action);
        match iteration {
            PostIterationOutcome::Preview(preview) => built_offers_preview.push(preview),
            PostIterationOutcome::Failure(failure) => {
                publish_failures += 1;
                post_results.push(failure.to_venue_result(&ctx.publish_venue));
            }
            PostIterationOutcome::Success(success) => {
                if !success.success {
                    publish_failures += 1;
                }
                let venue_result = success.to_venue_result();
                if let Some(record) = success.persist_record {
                    persist_records.push(record);
                }
                post_results.push(venue_result);
            }
        }
    }

    persist_post_records_if_enabled(
        &ctx.program.home_dir,
        request.persist_results,
        request.dry_run,
        &persist_records,
    )?;

    let payload = json!({
        "market_id": ctx.market.market_id,
        "pair": format!("{}:{}", ctx.market.base_asset, ctx.market.quote_asset),
        "resolved_base_asset_id": ctx.resolved_base_asset_id,
        "resolved_quote_asset_id": ctx.resolved_quote_asset_id,
        "network": ctx.program.network,
        "size_base_units": request.size_base_units,
        "repeat": request.repeat,
        "publish_venue": ctx.publish_venue,
        "dexie_base_url": ctx.dexie_base_url,
        "splash_base_url": if ctx.publish_venue == "splash" { Value::String(ctx.splash_base_url.clone()) } else { Value::Null },
        "drop_only": request.drop_only,
        "claim_rewards": request.claim_rewards,
        "dry_run": request.dry_run,
        "publish_attempts": post_results.len(),
        "publish_failures": publish_failures,
        "built_offers_preview": built_offers_preview,
        "bootstrap_actions": bootstrap_actions,
        "results": post_results,
        "offer_fee_mojos": ctx.offer_fee_mojos,
        "offer_fee_source": ctx.offer_fee_source,
        "execution_backend": "signer",
        "signer_path": true,
    });
    let exit_code = build_and_post_exit_code(publish_failures);
    let output = format_build_and_post_output(&payload, request.compact_json);
    Ok(BuildAndPostOfferResponse {
        exit_code,
        payload,
        output,
    })
}
