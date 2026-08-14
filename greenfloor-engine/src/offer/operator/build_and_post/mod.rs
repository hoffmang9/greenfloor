mod context;
mod create;
mod iteration;
mod post_batch;
mod publish;
mod types;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::adapters::{DexieClient, SplashClient};
use crate::async_boundary::BuildAndPostOfferFuture;
use crate::config::ManagerProgramConfig;
use crate::error::{SignerError, SignerResult};
use crate::offer::operator::UniqueMakerPinSession;
use crate::offer::request::normalize_offer_side;
use crate::paths::resolve_cats_config_path;
use crate::storage::{
    persist_offer_post_records, state_db_path_for_home, upsert_offer_post_record,
    OfferPostPersistRecord, SqliteStore,
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
use types::{build_and_post_exit_code, post_completed_outcome};

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

/// Program/markets config paths for managed ensure / build-and-post callers.
#[derive(Debug, Clone)]
pub struct OperatorConfigPaths {
    pub program_path: PathBuf,
    pub markets_path: PathBuf,
    pub testnet_markets_path: Option<PathBuf>,
}

impl BuildAndPostOfferRequest {
    /// Daemon / ensure-size post request (drop-only, single repeat).
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
    session: &mut UniqueMakerPinSession,
) -> SignerResult<PostIterationBatch> {
    let mut batch = PostIterationBatch {
        post_results: Vec::new(),
        built_offers_preview: Vec::new(),
        bootstrap_actions: Vec::new(),
        publish_failures: 0,
        persist: PostPersistPayload::default(),
    };
    let emitter = PostBatchEmitter::new(ctx);
    for _ in 0..request.repeat {
        let (bootstrap_action, iteration) =
            run_post_iteration(request, ctx, dexie, splash, session).await?;
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
    publish_attempts: usize,
    publish_failures: u32,
    dry_run: bool,
}

/// Persist offer-post records collected during build-and-post, then emit audits.
///
/// Immediate per-iteration upsert is the primary path. This flush retries those
/// upserts (idempotent) so a transient store failure after a live venue publish
/// still writes offer state and watches. Audits always run: persist-after-publish
/// failures are emitted only when this retry also fails.
///
/// # Errors
///
/// Returns an error when sqlite writes fail.
pub(crate) fn flush_build_and_post_persist(
    store: &SqliteStore,
    artifacts: &BuildAndPostPersistArtifacts,
) -> SignerResult<()> {
    let persist_result = persist_offer_post_records(store, &artifacts.persist.persist_records);
    let mut failure_audits = artifacts.persist.failure_audits.clone();
    if persist_result.is_err() {
        failure_audits.extend(artifacts.persist.deferred_persist_failures.iter().cloned());
    }
    let emitter = PostBatchEmitter::new(&artifacts.ctx);
    emitter.flush_audits(store, &artifacts.persist.persist_records, &failure_audits)?;
    persist_result
}

/// Flush when a store is present, then emit completion. Persist retry failure is
/// never reported as venue success.
fn complete_build_and_post(
    store: Option<&SqliteStore>,
    artifacts: &BuildAndPostPersistArtifacts,
) -> SignerResult<()> {
    let persist_result = match store {
        Some(store) => flush_build_and_post_persist(store, artifacts),
        None => Ok(()),
    };
    PostBatchEmitter::new(&artifacts.ctx).trace_completed(
        post_completed_outcome(
            artifacts.publish_attempts,
            artifacts.publish_failures,
            persist_result.is_err(),
        ),
        artifacts.publish_attempts,
        artifacts.publish_failures,
        artifacts.dry_run,
    );
    persist_result
}

fn lock_cli_store(
    store: &Arc<Mutex<SqliteStore>>,
) -> SignerResult<std::sync::MutexGuard<'_, SqliteStore>> {
    store
        .lock()
        .map_err(|err| SignerError::Other(format!("CLI persist store lock: {err}")))
}

/// `preopened_cli_store`: home DB already opened by the CLI entry for session begin / persist.
async fn finish_build_and_post(
    request: &BuildAndPostOfferRequest,
    ctx: ResolvedBuildAndPostContext,
    persist: Option<&mut OfferPostPersistSink<'_>>,
    session: &mut UniqueMakerPinSession,
    preopened_cli_store: Option<Arc<Mutex<SqliteStore>>>,
) -> SignerResult<(BuildAndPostOfferResponse, BuildAndPostPersistArtifacts)> {
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

    // CLI posts own a connection for unique-pin begin + synchronous persist. Daemon
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
        session,
    )
    .await?;

    let publish_attempts = batch.post_results.len();
    let publish_failures = batch.publish_failures;
    let payload = build_and_post_payload(request, &ctx, &batch);
    let exit_code = build_and_post_exit_code(publish_failures);
    let artifacts = BuildAndPostPersistArtifacts {
        persist: batch.into_persist_payload(),
        ctx,
        publish_attempts,
        publish_failures,
        dry_run: request.run.dry_run,
    };
    Ok((BuildAndPostOfferResponse { exit_code, payload }, artifacts))
}

async fn build_and_post_offer_async(
    request: BuildAndPostOfferRequest,
) -> SignerResult<BuildAndPostOfferResponse> {
    let ctx = resolve_build_and_post_context(&request).await?;
    let target = PostEmitTarget::from_run(request.run.persist_results, request.run.dry_run);
    let market_id = ctx.gated.market_row.market_id.as_str();
    let needs_live = UniqueMakerPinSession::needs_live(
        request.run.dry_run,
        ctx.gated.market_row.unique_maker_coins,
        request.maker_reuse.as_ref(),
    );
    // One home-DB connection for live unique-pin begin and/or TraceAndStore persist.
    let cli_store = if needs_live || target == PostEmitTarget::TraceAndStore {
        Some(Arc::new(Mutex::new(SqliteStore::open(
            &state_db_path_for_home(&ctx.gated.program.home_dir),
        )?)))
    } else {
        None
    };
    let mut session = match cli_store.as_ref() {
        Some(arc) => {
            let store = lock_cli_store(arc)?;
            UniqueMakerPinSession::begin(
                &store,
                market_id,
                request.run.dry_run,
                ctx.gated.market_row.unique_maker_coins,
                request.maker_reuse.as_ref(),
            )?
        }
        None => UniqueMakerPinSession::inactive(),
    };
    let (response, artifacts) =
        finish_build_and_post(&request, ctx, None, &mut session, cli_store.clone()).await?;
    let persist_store = match cli_store.as_ref() {
        Some(arc) if target == PostEmitTarget::TraceAndStore => Some(lock_cli_store(arc)?),
        _ => None,
    };
    complete_build_and_post(persist_store.as_deref(), &artifacts)?;
    Ok(response)
}

/// Run build-and-post and return persist artifacts for caller-side completion.
///
/// Callers construct [`UniqueMakerPinSession`] via [`UniqueMakerPinSession::begin`] on their
/// held store (daemon/ensure: cycle write store; CLI: home-DB shared with persist).
///
/// # Errors
///
/// Returns an error if the operation fails.
pub(crate) async fn build_and_post_offer_with_persist_artifacts(
    request: BuildAndPostOfferRequest,
    persist: Option<&mut OfferPostPersistSink<'_>>,
    session: &mut UniqueMakerPinSession,
) -> SignerResult<(BuildAndPostOfferResponse, BuildAndPostPersistArtifacts)> {
    if request.size_base_units == 0 {
        return Err(SignerError::Other(
            "size_base_units must be positive".to_string(),
        ));
    }
    if request.repeat == 0 {
        return Err(SignerError::Other("repeat must be positive".to_string()));
    }
    let ctx = resolve_build_and_post_context(&request).await?;
    finish_build_and_post(&request, ctx, persist, session, None).await
}

/// Pin, persist, and flush on a cycle write store (daemon / `ensure_size`).
pub(crate) async fn build_and_post_offer_on_cycle_store(
    request: BuildAndPostOfferRequest,
    write_store: &crate::storage::CycleWriteStore,
    unique_maker_coins: bool,
) -> SignerResult<BuildAndPostOfferResponse> {
    let market_id = request
        .market_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| SignerError::Other("post requires market_id".to_string()))?;
    let mut session = write_store.sync(|store| {
        UniqueMakerPinSession::begin(
            store,
            market_id,
            request.run.dry_run,
            unique_maker_coins,
            request.maker_reuse.as_ref(),
        )
    })?;
    let persist_store = write_store.clone();
    let mut persist = move |record: &OfferPostPersistRecord| {
        persist_store.sync(|store| upsert_offer_post_record(store, record))
    };
    let target = PostEmitTarget::from_run(request.run.persist_results, request.run.dry_run);
    let (response, artifacts) =
        build_and_post_offer_with_persist_artifacts(request, Some(&mut persist), &mut session)
            .await?;
    let locked = if target == PostEmitTarget::TraceAndStore {
        Some(write_store.lock()?)
    } else {
        None
    };
    complete_build_and_post(locked.as_deref(), &artifacts)?;
    Ok(response)
}

#[cfg(test)]
impl BuildAndPostPersistArtifacts {
    fn for_test(persist: PostPersistPayload, ctx: ResolvedBuildAndPostContext) -> Self {
        Self {
            persist,
            ctx,
            publish_attempts: 0,
            publish_failures: 0,
            dry_run: false,
        }
    }
}

#[cfg(test)]
pub(crate) fn empty_persist_artifacts_for_test() -> BuildAndPostPersistArtifacts {
    BuildAndPostPersistArtifacts::for_test(
        PostPersistPayload::default(),
        context::sample_resolved_build_and_post_context(),
    )
}

#[cfg(test)]
mod unique_pin_session_tests {
    use super::*;
    use crate::offer::types::{OfferCancelFields, OfferExecutionMode};
    use crate::storage::{OfferCancelWrite, OfferListingWrite};
    use crate::test_support::build_and_post::unused_post_iteration_request;

    #[test]
    fn begin_inactive_when_unique_pin_gated_off() {
        let request = unused_post_iteration_request(true, None);
        let ctx = sample_resolved_build_and_post_context();
        assert!(!UniqueMakerPinSession::needs_live(
            request.run.dry_run,
            ctx.gated.market_row.unique_maker_coins,
            request.maker_reuse.as_ref(),
        ));
        assert!(!UniqueMakerPinSession::inactive().is_live());
    }

    #[test]
    fn begin_loads_bindings_from_home_store() {
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
        let session = UniqueMakerPinSession::begin(&store, "m1", false, true, None).expect("begin");
        assert!(session.is_live());
        assert!(session.has_exclude(&coin));
    }
}
