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
use crate::async_boundary::BuildAndPostOfferFuture;
use crate::error::{SignerError, SignerResult};
use crate::offer::operator::OfferOperatorTestOverrides;
use crate::storage::OfferPostPersistRecord;

use crate::operator_log::{offer_log_ref, LogContext, OFFER_POST_COMPLETED, OFFER_POST_ITERATION};
use context::{resolve_build_and_post_context, ResolvedBuildAndPostContext};
use iteration::run_post_iteration;
use publish::{persist_post_failure_if_enabled, persist_post_records_if_enabled};
use tracing::Level;
use types::{build_and_post_exit_code, PostIterationOutcome};

#[derive(Debug, Clone, Deserialize)]
pub struct BuildAndPostVenueOptions {
    #[serde(default)]
    pub drop_only: bool,
    #[serde(default)]
    pub claim_rewards: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuildAndPostRunOptions {
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default = "default_persist_results")]
    pub persist_results: bool,
}

#[must_use]
fn default_persist_results() -> bool {
    true
}

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
    #[serde(flatten)]
    pub venue: BuildAndPostVenueOptions,
    #[serde(flatten)]
    pub run: BuildAndPostRunOptions,
    /// When set, overrides ``pricing.side`` for bootstrap and offer construction (daemon buy/sell actions).
    pub action_side: Option<String>,
    #[serde(default)]
    pub test_overrides: OfferOperatorTestOverrides,
}

#[derive(Debug, Clone)]
pub struct BuildAndPostOfferResponse {
    pub exit_code: i32,
    pub payload: Value,
}

/// Build and post offer.
///
/// # Errors
///
/// Returns an error if the operation fails.
#[must_use]
pub fn build_and_post_offer(request: BuildAndPostOfferRequest) -> BuildAndPostOfferFuture {
    Box::pin(build_and_post_offer_async(request))
}

struct PostIterationBatch {
    post_results: Vec<Value>,
    built_offers_preview: Vec<Value>,
    bootstrap_actions: Vec<Value>,
    publish_failures: u32,
    persist_records: Vec<OfferPostPersistRecord>,
}

async fn run_post_iterations(
    request: &BuildAndPostOfferRequest,
    ctx: &ResolvedBuildAndPostContext,
    dexie: Option<&DexieClient>,
    splash: Option<&SplashClient>,
) -> SignerResult<PostIterationBatch> {
    let mut batch = PostIterationBatch {
        post_results: Vec::new(),
        built_offers_preview: Vec::new(),
        bootstrap_actions: Vec::new(),
        publish_failures: 0,
        persist_records: Vec::new(),
    };
    for _ in 0..request.repeat {
        let (bootstrap_action, iteration) = run_post_iteration(request, ctx, dexie, splash).await?;
        batch.bootstrap_actions.push(bootstrap_action);
        match iteration {
            PostIterationOutcome::Preview(preview) => {
                let offer_ref = preview
                    .get("offer_prefix")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                crate::trace_event!(
                    INFO,
                    LogContext::OFFER_POST,
                    OFFER_POST_ITERATION,
                    {
                        market_id = ctx.market.market_id.as_str(),
                        outcome = "preview",
                        publish_venue = ctx.publish_venue.as_str(),
                        offer_ref = offer_ref,
                    };
                    "offer post iteration"
                );
                batch.built_offers_preview.push(preview);
            }
            PostIterationOutcome::Failure(failure) => {
                batch.publish_failures += 1;
                persist_post_failure_if_enabled(
                    &ctx.program.home_dir,
                    request.run.persist_results,
                    request.run.dry_run,
                    &ctx.market.market_id,
                    &ctx.publish_venue,
                    &failure.error,
                    None,
                )?;
                batch
                    .post_results
                    .push(failure.to_venue_result(&ctx.publish_venue));
            }
            PostIterationOutcome::Success(success) => {
                if success.success {
                    let offer_ref = success
                        .persist_record
                        .as_ref()
                        .map(|record| offer_log_ref(&record.offer_id))
                        .unwrap_or_default();
                    crate::trace_event!(
                        INFO,
                        LogContext::OFFER_POST,
                        OFFER_POST_ITERATION,
                        {
                            market_id = ctx.market.market_id.as_str(),
                            outcome = "success",
                            publish_venue = ctx.publish_venue.as_str(),
                            offer_ref = offer_ref.as_str(),
                        };
                        "offer post iteration"
                    );
                } else {
                    batch.publish_failures += 1;
                    let error = success
                        .result
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("publish_failed");
                    persist_post_failure_if_enabled(
                        &ctx.program.home_dir,
                        request.run.persist_results,
                        request.run.dry_run,
                        &ctx.market.market_id,
                        &ctx.publish_venue,
                        error,
                        success.persist_record.as_ref().map(|r| r.offer_id.as_str()),
                    )?;
                }
                let venue_result = success.to_venue_result();
                if let Some(record) = success.persist_record {
                    batch.persist_records.push(record);
                }
                batch.post_results.push(venue_result);
            }
        }
    }
    Ok(batch)
}

fn offer_post_completed_level(outcome: &str) -> Level {
    match outcome {
        "success" => Level::INFO,
        "failure" => Level::ERROR,
        _ => Level::WARN,
    }
}

fn trace_offer_post_completed(
    outcome: &str,
    market_id: &str,
    publish_attempts: usize,
    publish_failures: u32,
    dry_run: bool,
) {
    crate::event_at_level!(
        offer_post_completed_level(outcome),
        service = LogContext::OFFER_POST.service,
        event = OFFER_POST_COMPLETED,
        phase = LogContext::OFFER_POST.phase,
        market_id = market_id,
        outcome = outcome,
        publish_attempts = publish_attempts,
        publish_failures = publish_failures,
        dry_run = dry_run,
        "build-and-post-offer completed"
    );
}

fn build_and_post_payload(
    request: &BuildAndPostOfferRequest,
    ctx: &ResolvedBuildAndPostContext,
    batch: &PostIterationBatch,
) -> Value {
    json!({
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
        "drop_only": request.venue.drop_only,
        "claim_rewards": request.venue.claim_rewards,
        "dry_run": request.run.dry_run,
        "publish_attempts": batch.post_results.len(),
        "publish_failures": batch.publish_failures,
        "built_offers_preview": &batch.built_offers_preview,
        "bootstrap_actions": &batch.bootstrap_actions,
        "results": &batch.post_results,
        "offer_fee_mojos": ctx.offer_fee_mojos,
        "offer_fee_source": ctx.offer_fee_source,
    })
}

async fn build_and_post_offer_async(
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

    let dexie = if !request.run.dry_run && ctx.publish_venue == "dexie" {
        Some(DexieClient::new(ctx.dexie_base_url.clone()))
    } else {
        None
    };
    let splash = if !request.run.dry_run && ctx.publish_venue == "splash" {
        Some(SplashClient::new(ctx.splash_base_url.clone()))
    } else {
        None
    };

    let batch = run_post_iterations(&request, &ctx, dexie.as_ref(), splash.as_ref()).await?;

    persist_post_records_if_enabled(
        &ctx.program.home_dir,
        request.run.persist_results,
        request.run.dry_run,
        &batch.persist_records,
    )?;

    let payload = build_and_post_payload(&request, &ctx, &batch);
    let exit_code = build_and_post_exit_code(batch.publish_failures);
    let outcome = if batch.publish_failures == 0 {
        "success"
    } else if batch.publish_failures == u32::try_from(batch.post_results.len()).unwrap_or(u32::MAX)
    {
        "failure"
    } else {
        "partial_failure"
    };
    trace_offer_post_completed(
        outcome,
        ctx.market.market_id.as_str(),
        batch.post_results.len(),
        batch.publish_failures,
        request.run.dry_run,
    );
    Ok(BuildAndPostOfferResponse { exit_code, payload })
}
