use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::json;
use tracing::Level;

use crate::config::{MarketConfig, SignerConfig};
use crate::cycle::{
    plan_parallel_managed_dispatch, ParallelReservationContext, PlannedAction,
    SpendableAssetProfile, StrategyActionSellCountInput,
};

use super::{parallel_max_workers, reservation_release_status};
use crate::daemon::market_context::MarketCycleContext;
use crate::error::{SignerError, SignerResult};
use crate::offer::request::normalize_offer_side;
use crate::operator_log::{LogContext, PARALLEL_OFFER_DISPATCH};

use super::coordinator::{OfferReservationCoordinator, ReservationAcquireResult};
use super::managed_post::{post_managed_planned_action_owned, ManagedPostContext};
use super::reservation_ctx::{
    parallel_reservation_asset_ids, parallel_reservation_context, reservation_wallet_id,
};
use super::OfferDispatchOutput;

use crate::daemon::coinset_spendable::coinset_spendable_profiles_for_signer;

struct ParallelPostJob {
    action: PlannedAction,
    requested_amounts: BTreeMap<String, i64>,
    available_amounts: BTreeMap<String, i64>,
}

struct ParallelDispatchSetup {
    coordinator: Arc<OfferReservationCoordinator>,
    wallet_id: String,
    jobs: Vec<ParallelPostJob>,
    max_workers: usize,
    skip_items: Vec<StrategyActionSellCountInput>,
}

async fn resolve_parallel_spendable_profiles(
    ctx: &MarketCycleContext<'_>,
    market: &MarketConfig,
    reservation_ctx: &ParallelReservationContext,
) -> SignerResult<BTreeMap<String, SpendableAssetProfile>> {
    #[cfg(test)]
    if let Some(profiles) = &ctx.dispatch.test_controls.offer_dispatch.spendable_profiles {
        return Ok(profiles.clone());
    }
    let asset_ids = parallel_reservation_asset_ids(reservation_ctx);
    let signer = ctx.resources.signer_for_execution()?;
    coinset_spendable_profiles_for_signer(
        &ctx.resources.network,
        signer,
        &market.receive_address,
        &asset_ids,
    )
    .await
}

fn prepare_parallel_dispatch(
    ctx: &MarketCycleContext<'_>,
    signer_config: &SignerConfig,
    market: &MarketConfig,
    expanded: &[PlannedAction],
    reservation_ctx: &ParallelReservationContext,
    spendable_profiles: &BTreeMap<String, SpendableAssetProfile>,
) -> SignerResult<ParallelDispatchSetup> {
    let resources = ctx.resources;
    let program = resources.program();
    let batch_plan = plan_parallel_managed_dispatch(expanded, reservation_ctx, spendable_profiles)?;
    let ttl = crate::config::u64_to_i64(
        program.runtime_reservation_ttl_seconds,
        "runtime.reservation_ttl_seconds",
    )?;
    let coordinator = Arc::new(OfferReservationCoordinator::new(
        ctx.dispatch.write_store.clone(),
        Some(ttl),
    ));
    let _ = coordinator.expire_stale();
    let wallet_id = reservation_wallet_id(signer_config);

    ctx.dispatch.write_store.sync(|store| {
        LogContext::MARKET_CYCLE.dual_audit(
            store,
            Level::DEBUG,
            "parallel offer dispatch planned",
            PARALLEL_OFFER_DISPATCH,
            &json!({
                "market_id": market.market_id,
                "planned_count": expanded.len(),
                "queued_count": batch_plan.queue.len(),
                "workers": parallel_max_workers(
                    batch_plan.queue.len(),
                    program.runtime_offer_parallelism_max_workers
                ),
            }),
            Some(&market.market_id),
        )
    })?;

    let mut skip_items = Vec::new();
    for skip in &batch_plan.skip_items {
        let action = &expanded[skip.submit_index];
        skip_items.push(StrategyActionSellCountInput {
            size: action.size,
            side: normalize_offer_side(&action.side).to_string(),
            counts_as_executed: false,
        });
    }

    let jobs: Vec<ParallelPostJob> = batch_plan
        .queue
        .into_iter()
        .map(|item| ParallelPostJob {
            action: expanded[item.submit_index].clone(),
            requested_amounts: item.requested_amounts,
            available_amounts: item.available_amounts,
        })
        .collect();
    let max_workers =
        parallel_max_workers(jobs.len(), program.runtime_offer_parallelism_max_workers);

    Ok(ParallelDispatchSetup {
        coordinator,
        wallet_id,
        jobs,
        max_workers,
        skip_items,
    })
}

async fn run_parallel_post_jobs(
    post_ctx: Arc<ManagedPostContext>,
    market: &MarketConfig,
    setup: ParallelDispatchSetup,
) -> SignerResult<(u64, Vec<StrategyActionSellCountInput>)> {
    let ParallelDispatchSetup {
        coordinator,
        wallet_id,
        jobs,
        max_workers,
        mut skip_items,
    } = setup;

    let market = market.clone();
    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_workers));
    let mut handles = Vec::with_capacity(jobs.len());

    for job in jobs {
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|err| SignerError::Other(format!("parallel semaphore failed: {err}")))?;
        let coordinator = coordinator.clone();
        let post_ctx = Arc::clone(&post_ctx);
        let market = market.clone();
        let market_id = market.market_id.clone();
        let wallet_id = wallet_id.clone();

        handles.push(tokio::spawn(async move {
            let _permit = permit;
            let acquired = coordinator.try_acquire(
                &market_id,
                &wallet_id,
                &job.requested_amounts,
                &job.available_amounts,
            );
            let counts_as_executed = match acquired {
                Ok(ReservationAcquireResult::Acquired { reservation_id }) => {
                    let action = job.action.clone();
                    let post_result =
                        post_managed_planned_action_owned(post_ctx, market, action).await?;
                    let release_status = reservation_release_status(post_result);
                    let _ = coordinator.release(&reservation_id, release_status);
                    post_result
                }
                Ok(ReservationAcquireResult::Rejected { reason }) => {
                    return Err(SignerError::ReservationContention(reason.to_string()));
                }
                Err(err) => return Err(err),
            };
            Ok::<(PlannedAction, bool), SignerError>((job.action, counts_as_executed))
        }));
    }

    let mut executed = 0_u64;
    for handle in handles {
        let (action, counts_as_executed) = handle
            .await
            .map_err(|err| SignerError::Other(format!("parallel worker join failed: {err}")))?
            .map_err(|err| {
                if err.is_sqlite_fatal() {
                    err
                } else {
                    SignerError::Other(format!("parallel worker failed: {err}"))
                }
            })?;
        if counts_as_executed {
            executed += 1;
        }
        skip_items.push(StrategyActionSellCountInput {
            size: action.size,
            side: normalize_offer_side(&action.side).to_string(),
            counts_as_executed,
        });
    }

    Ok((executed, skip_items))
}

pub async fn execute_actions_parallel(
    ctx: &MarketCycleContext<'_>,
    signer_config: &SignerConfig,
    market: &MarketConfig,
    expanded: &[PlannedAction],
) -> SignerResult<OfferDispatchOutput> {
    #[cfg(test)]
    if let Some(result) =
        super::test_overrides::parallel_dispatch_result(&ctx.dispatch.test_controls.offer_dispatch)
    {
        return result;
    }

    let reservation_ctx =
        parallel_reservation_context(signer_config, &ctx.resources.program().network, market, 0)
            .await?;

    let spendable_profiles =
        resolve_parallel_spendable_profiles(ctx, market, &reservation_ctx).await?;

    let setup = prepare_parallel_dispatch(
        ctx,
        signer_config,
        market,
        expanded,
        &reservation_ctx,
        &spendable_profiles,
    )?;

    if setup.jobs.is_empty() {
        return Ok(OfferDispatchOutput {
            executed_count: 0,
            newly_executed_sell_counts: crate::cycle::executed_sell_offer_counts_by_size(
                &setup.skip_items,
            ),
        });
    }

    let post_ctx = Arc::new(ManagedPostContext::from_market_cycle(ctx));
    let (executed, action_items) = run_parallel_post_jobs(post_ctx, market, setup).await?;

    Ok(OfferDispatchOutput {
        executed_count: executed,
        newly_executed_sell_counts: crate::cycle::executed_sell_offer_counts_by_size(&action_items),
    })
}
