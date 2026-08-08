mod context;
mod create;
mod iteration;
mod post_batch;
mod publish;
mod types;

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::adapters::{DexieClient, SplashClient};
use crate::async_boundary::BuildAndPostOfferFuture;
use crate::config::ManagerProgramConfig;
use crate::error::{SignerError, SignerResult};
use crate::offer::operator::{
    load_binding_maker_coin_ids, needs_live_unique_pin, session_excludes_for_unique_pin,
};
use crate::offer::request::normalize_offer_side;
use crate::paths::resolve_cats_config_path;
use crate::storage::{
    state_db_path_for_home, upsert_offer_post_record, OfferPostPersistRecord, SqliteStore,
};

use context::resolve_build_and_post_context;
pub(crate) use context::ResolvedBuildAndPostContext;
#[cfg(test)]
pub(crate) use context::{
    sample_resolved_build_and_post_context, signer_denomination_test_context,
};
use iteration::run_post_iteration;
use post_batch::{
    apply_post_iteration_outcome, PostBatchEmitter, PostEmitTarget, PostIterationBatch,
    PostPersistPayload,
};
use types::build_and_post_exit_code;

/// Synchronously persist a successful venue post before its outcome is recorded.
///
/// The hook is called only after an iteration's async venue work completes; it is
/// never invoked while an async operation is pending.
pub(crate) type OfferPostPersistSink<'a> =
    dyn FnMut(&OfferPostPersistRecord) -> SignerResult<()> + Send + 'a;

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
    pub cats_path: Option<PathBuf>,
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
    /// Reuse an unspent soft-expiry maker via `PresplitExisting`.
    #[serde(default)]
    pub maker_reuse: Option<crate::offer::types::PresplitMakerReuse>,
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
    pub cats_path: Option<PathBuf>,
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
    pub maker_reuse: Option<crate::offer::types::PresplitMakerReuse>,
}

/// Program/markets config paths for managed ensure / build-and-post callers.
#[derive(Debug, Clone)]
pub struct OperatorConfigPaths {
    pub program_path: PathBuf,
    pub markets_path: PathBuf,
    pub testnet_markets_path: Option<PathBuf>,
}

impl BuildAndPostOfferRequestParts {
    /// Shared parts for daemon/ensure size-N posts (`drop_only`, single repeat).
    #[must_use]
    pub fn for_ensure_size(
        paths: &OperatorConfigPaths,
        program: &ManagerProgramConfig,
        network: impl Into<String>,
        market_id: impl Into<String>,
        size_base_units: u64,
        side: impl Into<String>,
    ) -> Self {
        Self {
            cats_path: Some(resolve_cats_config_path(&paths.markets_path, None)),
            program_path: paths.program_path.clone(),
            markets_path: paths.markets_path.clone(),
            testnet_markets_path: paths.testnet_markets_path.clone(),
            network: network.into(),
            market_id: Some(market_id.into()),
            pair: None,
            size_base_units,
            repeat: 1,
            publish_venue: Some(program.offer_publish_venue.clone()),
            dexie_base_url: Some(program.dexie_api_base.clone()),
            splash_base_url: Some(program.splash_api_base.clone()),
            venue: BuildAndPostVenueOptions {
                drop_only: true,
                claim_rewards: false,
            },
            run: BuildAndPostRunOptions {
                dry_run: program.runtime_dry_run,
                persist_results: true,
            },
            action_side: Some(normalize_offer_side(&side.into()).to_string()),
            maker_reuse: None,
        }
    }
}

impl BuildAndPostOfferRequest {
    #[must_use]
    pub fn from_parts(parts: BuildAndPostOfferRequestParts) -> Self {
        Self {
            program_path: parts.program_path,
            markets_path: parts.markets_path,
            testnet_markets_path: parts.testnet_markets_path,
            cats_path: parts.cats_path,
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
            maker_reuse: parts.maker_reuse,
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
    mut persist: Option<&mut OfferPostPersistSink<'_>>,
    session_excludes: &mut HashSet<String>,
) -> SignerResult<PostIterationBatch> {
    let mut batch = PostIterationBatch {
        post_results: Vec::new(),
        built_offers_preview: Vec::new(),
        bootstrap_actions: Vec::new(),
        publish_failures: 0,
        persist: PostPersistPayload {
            persist_records: Vec::new(),
            failure_audits: Vec::new(),
        },
    };
    let emitter = PostBatchEmitter::new(ctx);
    for _ in 0..request.repeat {
        let (bootstrap_action, iteration) =
            run_post_iteration(request, ctx, dexie, splash, session_excludes).await?;
        batch.bootstrap_actions.push(bootstrap_action);
        // Reborrow the sink each iteration (cannot move Option<&mut _> in a loop).
        apply_post_iteration_outcome(
            target,
            &emitter,
            iteration,
            &mut batch,
            persist.as_deref_mut(),
        );
    }
    Ok(batch)
}

fn build_and_post_payload(
    request: &BuildAndPostOfferRequest,
    ctx: &ResolvedBuildAndPostContext,
    batch: &PostIterationBatch,
) -> Value {
    json!({
        "market_id": ctx.gated.market_row.market_id,
        "pair": format!("{}:{}", ctx.gated.market_row.base_asset, ctx.gated.market_row.quote_asset),
        "resolved_base_asset_id": ctx.offer_assets.base_asset_id,
        "resolved_quote_asset_id": ctx.offer_assets.quote_asset_id,
        "network": ctx.gated.operator_network,
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

#[derive(Clone)]
pub(crate) struct BuildAndPostPersistArtifacts {
    pub persist: PostPersistPayload,
    pub ctx: ResolvedBuildAndPostContext,
}

/// Persist offer-post records collected during build-and-post.
///
/// # Errors
///
/// Returns an error when sqlite writes fail.
pub(crate) fn flush_build_and_post_persist(
    store: &SqliteStore,
    artifacts: &BuildAndPostPersistArtifacts,
) -> SignerResult<()> {
    let emitter = PostBatchEmitter::new(&artifacts.ctx);
    emitter.flush_audits(
        store,
        &artifacts.persist.persist_records,
        &artifacts.persist.failure_audits,
    )
}

fn lock_cli_store(store: &Arc<Mutex<SqliteStore>>) -> SignerResult<std::sync::MutexGuard<'_, SqliteStore>> {
    store
        .lock()
        .map_err(|err| SignerError::Other(format!("CLI persist store lock: {err}")))
}

/// `preopened_cli_store`: home DB already opened by the CLI entry for binding load.
async fn finish_build_and_post(
    request: &BuildAndPostOfferRequest,
    ctx: ResolvedBuildAndPostContext,
    persist: Option<&mut OfferPostPersistSink<'_>>,
    binding_excludes: Vec<String>,
    preopened_cli_store: Option<Arc<Mutex<SqliteStore>>>,
) -> SignerResult<(
    BuildAndPostOfferResponse,
    Option<BuildAndPostPersistArtifacts>,
)> {
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

    // CLI posts own a connection for binding seed + synchronous persist. Daemon
    // callers supply their cycle write store instead, avoiding a side connection.
    let cli_store = match preopened_cli_store {
        Some(store) => Some(store),
        None if target == PostEmitTarget::TraceAndStore && persist.is_none() => {
            Some(Arc::new(Mutex::new(SqliteStore::open(
                &state_db_path_for_home(&ctx.gated.program.home_dir),
            )?)))
        }
        None => None,
    };
    let mut session_excludes = session_excludes_for_unique_pin(
        request.run.dry_run,
        ctx.gated.market_row.unique_maker_coins,
        request.maker_reuse.as_ref(),
        &binding_excludes,
    );
    let mut cli_persist = move |record: &OfferPostPersistRecord| {
        let store = lock_cli_store(
            cli_store
                .as_ref()
                .expect("CLI persist sink requires a SQLite store"),
        )?;
        upsert_offer_post_record(&store, record)
    };
    let persist = if target == PostEmitTarget::TraceAndStore {
        match persist {
            Some(persist) => Some(persist),
            None => Some(&mut cli_persist as &mut OfferPostPersistSink<'_>),
        }
    } else {
        None
    };
    let batch = run_post_iterations(
        request,
        &ctx,
        target,
        dexie.as_ref(),
        splash.as_ref(),
        persist,
        &mut session_excludes,
    )
    .await?;

    let payload = build_and_post_payload(request, &ctx, &batch);
    let exit_code = build_and_post_exit_code(batch.publish_failures);
    let outcome = if batch.publish_failures == 0 {
        "success"
    } else if batch.publish_failures == u32::try_from(batch.post_results.len()).unwrap_or(u32::MAX)
    {
        "failure"
    } else {
        "partial_failure"
    };
    PostBatchEmitter::new(&ctx).trace_completed(
        outcome,
        batch.post_results.len(),
        batch.publish_failures,
        request.run.dry_run,
    );

    let persist_artifacts = if target == PostEmitTarget::TraceAndStore {
        Some(BuildAndPostPersistArtifacts {
            persist: batch.into_persist_payload(),
            ctx: ctx.clone(),
        })
    } else {
        None
    };

    Ok((
        BuildAndPostOfferResponse { exit_code, payload },
        persist_artifacts,
    ))
}

async fn build_and_post_offer_async(
    request: BuildAndPostOfferRequest,
) -> SignerResult<BuildAndPostOfferResponse> {
    let ctx = resolve_build_and_post_context(&request).await?;
    let target = PostEmitTarget::from_run(request.run.persist_results, request.run.dry_run);
    let live_pin = needs_live_unique_pin(
        request.run.dry_run,
        ctx.gated.market_row.unique_maker_coins,
        request.maker_reuse.as_ref(),
    );
    // One home-DB connection for binding seed, iteration persist, and artifact flush.
    let cli_store = if live_pin || target == PostEmitTarget::TraceAndStore {
        Some(Arc::new(Mutex::new(SqliteStore::open(
            &state_db_path_for_home(&ctx.gated.program.home_dir),
        )?)))
    } else {
        None
    };
    let binding_excludes = if live_pin {
        let store = lock_cli_store(cli_store.as_ref().expect("opened for live unique pin"))?;
        load_binding_maker_coin_ids(&store, &ctx.gated.market_row.market_id)?
    } else {
        Vec::new()
    };
    let (response, artifacts) =
        finish_build_and_post(&request, ctx, None, binding_excludes, cli_store.clone()).await?;
    if let Some(artifacts) = artifacts {
        let store = lock_cli_store(cli_store.as_ref().expect("TraceAndStore opened CLI store"))?;
        flush_build_and_post_persist(&store, &artifacts)?;
    }
    Ok(response)
}

/// Run build-and-post and return optional persist artifacts for caller-side flush.
///
/// Callers preload `binding_excludes` (daemon/ensure: cycle write store; CLI entry loads
/// from the same home-DB connection used for persist). Empty is correct when unique pin
/// is gated off.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub(crate) async fn build_and_post_offer_with_persist_artifacts(
    request: BuildAndPostOfferRequest,
    persist: Option<&mut OfferPostPersistSink<'_>>,
    binding_excludes: Vec<String>,
) -> SignerResult<(
    BuildAndPostOfferResponse,
    Option<BuildAndPostPersistArtifacts>,
)> {
    if request.size_base_units == 0 {
        return Err(SignerError::Other(
            "size_base_units must be positive".to_string(),
        ));
    }
    if request.repeat == 0 {
        return Err(SignerError::Other("repeat must be positive".to_string()));
    }
    let ctx = resolve_build_and_post_context(&request).await?;
    finish_build_and_post(&request, ctx, persist, binding_excludes, None).await
}

#[cfg(test)]
pub(crate) fn empty_persist_artifacts_for_test() -> BuildAndPostPersistArtifacts {
    BuildAndPostPersistArtifacts {
        persist: PostPersistPayload {
            persist_records: Vec::new(),
            failure_audits: Vec::new(),
        },
        ctx: context::sample_resolved_build_and_post_context(),
    }
}

#[cfg(test)]
mod binding_excludes_tests {
    use super::*;
    use crate::offer::types::{OfferCancelFields, OfferExecutionMode};
    use crate::storage::{OfferCancelWrite, OfferListingWrite};
    use crate::test_support::build_and_post::unused_post_iteration_request;

    #[test]
    fn session_excludes_skip_when_unique_pin_gated_off() {
        let request = unused_post_iteration_request(true, None);
        let ctx = sample_resolved_build_and_post_context();
        let seeded = session_excludes_for_unique_pin(
            request.run.dry_run,
            ctx.gated.market_row.unique_maker_coins,
            request.maker_reuse.as_ref(),
            &["aa".repeat(32)],
        );
        assert!(seeded.is_empty());
    }

    #[test]
    fn load_binding_maker_coin_ids_from_home_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SqliteStore::open(&state_db_path_for_home(dir.path())).expect("open");
        let coin = "aa".repeat(32);
        let fields =
            OfferCancelFields::from_presplit_build(coin.clone(), "bb".repeat(32), "cc".repeat(32));
        store
            .upsert_offer_state_with_metadata_at(
                "o1",
                "m1",
                "open",
                None,
                "2026-01-01T00:00:00Z",
                OfferCancelWrite {
                    fields: Some(&fields),
                    execution_mode: Some(OfferExecutionMode::Direct),
                    listing: OfferListingWrite {
                        publish_venue: Some("dexie"),
                        listing_expires_at: None,
                        size_base_units: Some(10),
                        offer_nonce: None,
                        offer_side: Some("sell"),
                    },
                    ..OfferCancelWrite::default()
                },
            )
            .expect("upsert");
        let ids = load_binding_maker_coin_ids(&store, "m1").expect("load");
        assert_eq!(ids, vec![coin]);
    }
}
