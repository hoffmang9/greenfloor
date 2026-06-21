mod context;
mod create;
mod iteration;
mod post_batch;
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
use crate::storage::{state_db_path_for_home, SqliteStore};

use context::{resolve_build_and_post_context, ResolvedBuildAndPostContext};
use iteration::run_post_iteration;
use post_batch::{
    apply_post_iteration_outcome, flush_post_batch, trace_offer_post_completed, PostEmitTarget,
    PostIterationBatch,
};
use types::build_and_post_exit_code;

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
    #[cfg(test)]
    #[serde(default, skip)]
    pub test_overrides: crate::offer::operator::BuildOfferTestOverrides,
}

/// Shared fields for constructing a [`BuildAndPostOfferRequest`] from CLI or daemon callers.
#[derive(Debug, Clone)]
pub struct BuildAndPostOfferRequestParts {
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
    pub venue: BuildAndPostVenueOptions,
    pub run: BuildAndPostRunOptions,
    pub action_side: Option<String>,
}

impl BuildAndPostOfferRequest {
    #[must_use]
    pub fn from_parts(parts: BuildAndPostOfferRequestParts) -> Self {
        Self {
            program_path: parts.program_path,
            markets_path: parts.markets_path,
            testnet_markets_path: parts.testnet_markets_path,
            network: parts.network,
            market_id: parts.market_id,
            pair: parts.pair,
            size_base_units: parts.size_base_units,
            repeat: parts.repeat,
            publish_venue: parts.publish_venue,
            dexie_base_url: parts.dexie_base_url,
            splash_base_url: parts.splash_base_url,
            venue: parts.venue,
            run: parts.run,
            action_side: parts.action_side,
            #[cfg(test)]
            test_overrides: crate::offer::operator::BuildOfferTestOverrides::default(),
        }
    }
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

async fn run_post_iterations(
    request: &BuildAndPostOfferRequest,
    ctx: &ResolvedBuildAndPostContext,
    target: PostEmitTarget,
    dexie: Option<&DexieClient>,
    splash: Option<&SplashClient>,
) -> SignerResult<PostIterationBatch> {
    let mut batch = PostIterationBatch {
        post_results: Vec::new(),
        built_offers_preview: Vec::new(),
        bootstrap_actions: Vec::new(),
        publish_failures: 0,
        persist_records: Vec::new(),
        failure_audits: Vec::new(),
    };
    for _ in 0..request.repeat {
        let (bootstrap_action, iteration) = run_post_iteration(request, ctx, dexie, splash).await?;
        batch.bootstrap_actions.push(bootstrap_action);
        apply_post_iteration_outcome(target, ctx, iteration, &mut batch);
    }
    Ok(batch)
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
    let target = PostEmitTarget::from_run(request.run.persist_results, request.run.dry_run);

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

    let batch =
        run_post_iterations(&request, &ctx, target, dexie.as_ref(), splash.as_ref()).await?;

    if target == PostEmitTarget::TraceAndStore {
        let store = SqliteStore::open(&state_db_path_for_home(&ctx.program.home_dir))?;
        flush_post_batch(&store, &ctx, &batch.persist_records, &batch.failure_audits)?;
    }

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
