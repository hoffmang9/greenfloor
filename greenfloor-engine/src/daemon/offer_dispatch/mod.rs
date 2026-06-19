//! Managed offer dispatch for the daemon strategy phase (sequential and parallel).

mod coordinator;
mod managed_post;
mod parallel;
mod reservation_ctx;
mod sequential;
mod test_hooks;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde_json::json;
use tracing::Level;

use crate::async_boundary::StrategyDispatchFuture;
use crate::config::{is_signer_execution_soft_skip, signer_execution_skip_reason, MarketConfig};
use crate::cycle::{expand_planned_actions, parallel_managed_dispatch_enabled, PlannedAction};
use crate::error::{SignerError, SignerResult};
use crate::operator_log::{
    operator_audit, AuditDurability, EmitMode, LogContext, OFFER_PARALLEL_FALLBACK,
    STRATEGY_EXEC_SKIPPED_NO_SIGNER,
};
use crate::storage::SqliteStore;

use super::market_context::MarketCycleContext;

#[derive(Debug, Clone)]
pub struct OfferDispatchOutput {
    pub executed_count: u64,
    pub newly_executed_sell_counts: BTreeMap<i64, i64>,
}

pub(crate) fn is_parallel_dispatch_transient_signer_error(err: &SignerError) -> bool {
    err.is_parallel_dispatch_transient()
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
        Err(err) if is_parallel_dispatch_transient_signer_error(&err) => {
            ParallelDispatchDecision::FallbackTransient(err)
        }
        Err(err) => ParallelDispatchDecision::Fatal(err),
    }
}

pub(crate) fn record_parallel_fallback_audit(
    store: &SqliteStore,
    market_id: &str,
    err: &SignerError,
) -> SignerResult<()> {
    operator_audit(
        Some(store),
        LogContext::MARKET_CYCLE,
        EmitMode::dual(Level::WARN, "parallel offer dispatch fallback"),
        OFFER_PARALLEL_FALLBACK,
        &json!({
            "market_id": market_id,
            "error": err.to_string(),
            "reason": "reservation_parallel_path_failed",
        }),
        Some(market_id),
        AuditDurability::Required,
    )
}

pub fn execute_strategy_actions<'a>(
    store: &'a SqliteStore,
    ctx: &'a MarketCycleContext<'_>,
    market: &'a MarketConfig,
    actions: &'a [PlannedAction],
) -> StrategyDispatchFuture<'a> {
    Box::pin(execute_strategy_actions_async(store, ctx, market, actions))
}

async fn execute_strategy_actions_async(
    store: &SqliteStore,
    ctx: &MarketCycleContext<'_>,
    market: &MarketConfig,
    actions: &[PlannedAction],
) -> SignerResult<OfferDispatchOutput> {
    let signer_config = match ctx.resources.signer_for_execution() {
        Err(err) if is_signer_execution_soft_skip(&err) => {
            operator_audit(
                Some(store),
                LogContext::MARKET_CYCLE,
                EmitMode::dual(Level::WARN, "strategy execution skipped without signer"),
                STRATEGY_EXEC_SKIPPED_NO_SIGNER,
                &json!({
                    "market_id": market.market_id,
                    "planned_count": actions.len(),
                    "reason": signer_execution_skip_reason(&err),
                }),
                Some(&market.market_id),
                AuditDurability::Required,
            )?;
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
            parallel::execute_actions_parallel(
                store,
                &ctx.dispatch.db_path,
                ctx.resources,
                signer_config,
                market,
                &expanded,
            )
            .await,
        ) {
            ParallelDispatchDecision::Success(output) => return Ok(output),
            ParallelDispatchDecision::FallbackTransient(err) => {
                record_parallel_fallback_audit(store, &market.market_id, &err)?;
            }
            ParallelDispatchDecision::Fatal(err) => return Err(err),
        }
    }

    sequential::execute_actions_sequential(program, &ctx.resources.paths, market, &expanded).await
}
