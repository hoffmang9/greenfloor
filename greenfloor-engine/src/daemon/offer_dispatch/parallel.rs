use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use serde_json::json;

use crate::config::{ManagerProgramConfig, MarketConfig};
use crate::cycle::{
    expand_planned_actions, parallel_max_workers, plan_parallel_managed_dispatch,
    reservation_release_status, PlannedAction, StrategyActionSellCountInput,
};
use crate::error::{SignerError, SignerResult};
use crate::offer::request::normalize_offer_side;
use crate::storage::SqliteStore;

use super::coordinator::OfferReservationCoordinator;
use super::managed_post::post_managed_planned_action;
use super::reservation_ctx::{
    parallel_reservation_asset_ids, parallel_reservation_context, reservation_wallet_id,
};
use super::spendable::coinset_spendable_profiles_by_asset;
use super::OfferDispatchOutput;

struct ParallelPostJob {
    action: PlannedAction,
    requested_amounts: BTreeMap<String, i64>,
    available_amounts: BTreeMap<String, i64>,
}

pub async fn execute_actions_parallel(
    store: &SqliteStore,
    db_path: &Path,
    program: &ManagerProgramConfig,
    market: &MarketConfig,
    network: &str,
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
    actions: &[PlannedAction],
) -> SignerResult<OfferDispatchOutput> {
    let expanded = expand_planned_actions(actions);
    let reservation_ctx = parallel_reservation_context(program_path, market, 0).await?;
    let asset_ids = parallel_reservation_asset_ids(&reservation_ctx);
    let spendable_profiles = coinset_spendable_profiles_by_asset(
        network,
        &market.receive_address,
        &asset_ids,
    )
    .await?;
    let batch_plan =
        plan_parallel_managed_dispatch(&expanded, &reservation_ctx, &spendable_profiles);
    let coordinator = Arc::new(OfferReservationCoordinator::new(db_path, Some(300)));
    let _ = coordinator.expire_stale();
    let wallet_id = reservation_wallet_id(program_path)?;

    store.add_audit_event(
        "parallel_offer_dispatch",
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
    )?;

    let mut action_items = Vec::new();
    for skip in &batch_plan.skip_items {
        let action = &expanded[skip.submit_index];
        action_items.push(StrategyActionSellCountInput {
            size: action.size,
            side: normalize_offer_side(&action.side).to_string(),
            counts_as_executed: false,
        });
    }

    if batch_plan.queue.is_empty() {
        return Ok(OfferDispatchOutput {
            executed_count: 0,
            newly_executed_sell_counts: crate::cycle::executed_sell_offer_counts_by_size(
                &action_items,
            ),
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

    let max_workers = parallel_max_workers(jobs.len(), program.runtime_offer_parallelism_max_workers);
    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_workers));
    let mut handles = Vec::with_capacity(jobs.len());

    for job in jobs {
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|err| SignerError::Other(format!("parallel semaphore failed: {err}")))?;
        let coordinator = coordinator.clone();
        let program = program.clone();
        let market = market.clone();
        let program_path = program_path.to_path_buf();
        let markets_path = markets_path.to_path_buf();
        let testnet_markets_path = testnet_markets_path.map(Path::to_path_buf);
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
                Ok(acquired) if acquired.ok => {
                    let reservation_id = acquired.reservation_id.expect("reservation id");
                    let post_result = post_managed_planned_action(
                        &program,
                        &market,
                        &program_path,
                        &markets_path,
                        testnet_markets_path.as_deref(),
                        &job.action,
                    )
                    .await
                    .unwrap_or(false);
                    let release_status = reservation_release_status(post_result);
                    let _ = coordinator.release(&reservation_id, release_status);
                    post_result
                }
                _ => false,
            };
            (job.action, counts_as_executed)
        }));
    }

    let mut executed = 0_u64;
    for handle in handles {
        let (action, counts_as_executed) = handle
            .await
            .map_err(|err| SignerError::Other(format!("parallel worker join failed: {err}")))?;
        if counts_as_executed {
            executed += 1;
        }
        action_items.push(StrategyActionSellCountInput {
            size: action.size,
            side: normalize_offer_side(&action.side).to_string(),
            counts_as_executed,
        });
    }

    Ok(OfferDispatchOutput {
        executed_count: executed,
        newly_executed_sell_counts: crate::cycle::executed_sell_offer_counts_by_size(&action_items),
    })
}
