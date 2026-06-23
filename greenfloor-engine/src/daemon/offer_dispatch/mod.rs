//! Managed offer dispatch for the daemon strategy phase (sequential and parallel).

mod coordinator;
mod managed_post;
mod parallel;
mod reservation_ctx;
mod sequential;
#[cfg(test)]
mod test_overrides;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde_json::json;
use tracing::Level;

use crate::async_boundary::StrategyDispatchFuture;
use crate::config::{
    is_signer_execution_soft_skip, signer_execution_skip_reason, ManagerProgramConfig, MarketConfig,
};
use crate::cycle::{expand_planned_actions, PlannedAction};

use crate::error::{SignerError, SignerResult};
use crate::operator_log::{LogContext, OFFER_PARALLEL_FALLBACK, STRATEGY_EXEC_SKIPPED_NO_SIGNER};
use crate::storage::CycleWriteStore;

use super::market_context::MarketCycleContext;

#[must_use]
fn parallel_managed_dispatch_enabled(program: &ManagerProgramConfig) -> bool {
    program.runtime_offer_parallelism_enabled && !program.runtime_dry_run
}

#[must_use]
pub(super) fn parallel_max_workers(submission_count: usize, configured_max: usize) -> usize {
    submission_count.min(configured_max.max(1))
}

#[must_use]
pub(super) fn reservation_release_status(is_executed: bool) -> &'static str {
    if is_executed {
        "released_success"
    } else {
        "released_failed"
    }
}

#[derive(Debug, Clone)]
pub struct OfferDispatchOutput {
    pub executed_count: u64,
    pub newly_executed_sell_counts: BTreeMap<i64, i64>,
}

/// Outcome of a parallel managed-offer dispatch attempt.
pub(crate) enum ParallelDispatchDecision {
    Success(OfferDispatchOutput),
    FallbackTransient(SignerError),
    Fatal(SignerError),
}

pub(crate) fn classify_parallel_dispatch(
    result: Result<OfferDispatchOutput, SignerError>,
) -> ParallelDispatchDecision {
    match result {
        Ok(output) => ParallelDispatchDecision::Success(output),
        Err(err) if err.is_parallel_dispatch_transient() => {
            ParallelDispatchDecision::FallbackTransient(err)
        }
        Err(err) => ParallelDispatchDecision::Fatal(err),
    }
}

pub(crate) fn record_parallel_fallback_audit(
    write_store: &CycleWriteStore,
    market_id: &str,
    err: &SignerError,
) -> SignerResult<()> {
    write_store.sync(|store| {
        LogContext::MARKET_CYCLE.dual_audit(
            store,
            Level::WARN,
            "parallel offer dispatch fallback",
            OFFER_PARALLEL_FALLBACK,
            &json!({
                "market_id": market_id,
                "error": err.to_string(),
                "reason": "reservation_parallel_path_failed",
            }),
            Some(market_id),
        )
    })
}

pub fn execute_strategy_actions<'a>(
    ctx: &'a MarketCycleContext<'_>,
    market: &'a MarketConfig,
    actions: &'a [PlannedAction],
) -> StrategyDispatchFuture<'a> {
    Box::pin(execute_strategy_actions_async(ctx, market, actions))
}

async fn execute_strategy_actions_async(
    ctx: &MarketCycleContext<'_>,
    market: &MarketConfig,
    actions: &[PlannedAction],
) -> SignerResult<OfferDispatchOutput> {
    let write_store = &ctx.dispatch.write_store;
    let signer_config = match ctx.resources.signer_for_execution() {
        Err(err) if is_signer_execution_soft_skip(&err) => {
            write_store.sync(|store| {
                LogContext::MARKET_CYCLE.dual_audit(
                    store,
                    Level::WARN,
                    "strategy execution skipped without signer",
                    STRATEGY_EXEC_SKIPPED_NO_SIGNER,
                    &json!({
                        "market_id": market.market_id,
                        "planned_count": actions.len(),
                        "reason": signer_execution_skip_reason(&err),
                    }),
                    Some(&market.market_id),
                )
            })?;
            return Ok(OfferDispatchOutput {
                executed_count: 0,
                newly_executed_sell_counts: BTreeMap::default(),
            });
        }
        Err(err) => return Err(err),
        Ok(signer) => signer,
    };

    let expanded = expand_planned_actions(actions);
    if expanded.is_empty() {
        return Ok(OfferDispatchOutput {
            executed_count: 0,
            newly_executed_sell_counts: BTreeMap::default(),
        });
    }

    let program = ctx.resources.program();
    if parallel_managed_dispatch_enabled(program) {
        match classify_parallel_dispatch(
            parallel::execute_actions_parallel(ctx, signer_config, market, &expanded).await,
        ) {
            ParallelDispatchDecision::Success(output) => return Ok(output),
            ParallelDispatchDecision::FallbackTransient(err) => {
                record_parallel_fallback_audit(write_store, &market.market_id, &err)?;
            }
            ParallelDispatchDecision::Fatal(err) => return Err(err),
        }
    }

    sequential::execute_actions_sequential(ctx, market, &expanded).await
}
