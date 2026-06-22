use crate::cycle::{
    enqueue_immediate_requeue, select_market_batch, should_use_market_slot_dispatch,
    StaleSweepProgress,
};
use crate::daemon::market_context::DaemonCycleResources;
use crate::daemon::markets::enabled_market_ids;
use crate::daemon::stale_sweep::detect_stale_open_offers_for_requeue;
use crate::error::SignerResult;
use crate::metrics::metric_u64_to_usize;
use crate::storage::{resolve_state_db_path, SqliteStore};

use super::cycle_types::CyclePlan;
use super::request::{DaemonDispatchState, DaemonRunOnceRequest};

fn apply_stale_requeues(dispatch_state: &mut DaemonDispatchState, requeue_market_ids: &[String]) {
    for market_id in requeue_market_ids {
        dispatch_state.immediate_requeue_ids =
            enqueue_immediate_requeue(&dispatch_state.immediate_requeue_ids, market_id);
    }
}

fn select_markets_for_cycle(
    enabled_market_ids: &[String],
    runtime_market_slot_count: u64,
    dispatch_state: &mut DaemonDispatchState,
) -> (Vec<String>, Vec<String>) {
    if should_use_market_slot_dispatch(
        enabled_market_ids.len(),
        metric_u64_to_usize(runtime_market_slot_count),
    ) {
        let selection = select_market_batch(
            enabled_market_ids,
            metric_u64_to_usize(runtime_market_slot_count),
            dispatch_state.cursor,
            &dispatch_state.immediate_requeue_ids,
        );
        dispatch_state.cursor = selection.cursor;
        dispatch_state.immediate_requeue_ids = selection.immediate_requeue_ids;
        return (
            selection.selected_market_ids,
            selection.consumed_immediate_requeues,
        );
    }
    if !enabled_market_ids.is_empty() {
        dispatch_state.cursor %= enabled_market_ids.len();
    }
    (enabled_market_ids.to_vec(), Vec::new())
}

/// Build cycle plan.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn build_cycle_plan(
    request: &DaemonRunOnceRequest,
    resources: &DaemonCycleResources,
    store: &SqliteStore,
) -> SignerResult<CyclePlan> {
    let program = resources.program();
    let db_path = resolve_state_db_path(&program.home_dir, request.state_db_override.as_deref());
    let previous_xch_price_usd = store.get_latest_xch_price_snapshot()?;
    let enabled_market_ids = enabled_market_ids(&resources.markets);

    let stale_open_sweep = if enabled_market_ids.is_empty() {
        StaleSweepProgress::default()
    } else {
        detect_stale_open_offers_for_requeue(store, &resources.dexie, &enabled_market_ids).await?
    };

    let runtime_market_slot_count = program.runtime_market_slot_count;
    let runtime_dry_run = program.runtime_dry_run;
    let mut dispatch_state = request.dispatch_state.clone();
    apply_stale_requeues(&mut dispatch_state, &stale_open_sweep.requeue_market_ids);
    let (selected_market_ids, consumed_immediate_requeues) = select_markets_for_cycle(
        &enabled_market_ids,
        runtime_market_slot_count,
        &mut dispatch_state,
    );

    Ok(CyclePlan {
        enabled_market_ids,
        selected_market_ids,
        consumed_immediate_requeues,
        dispatch_state,
        stale_open_sweep,
        configured_market_slot_count: runtime_market_slot_count,
        runtime_dry_run,
        db_path,
        previous_xch_price_usd,
        dexie_base_url: program.dexie_api_base.clone(),
        splash_base_url: program.splash_api_base.clone(),
        test_controls: request.test_controls.clone(),
    })
}
