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

use crate::config::MarketConfig;
use crate::cycle::{expand_planned_actions, parallel_managed_dispatch_enabled, PlannedAction};
use crate::error::{SignerError, SignerResult};
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

pub(crate) async fn record_parallel_fallback_audit(
    store: &SqliteStore,
    market_id: &str,
    err: &SignerError,
) -> SignerResult<()> {
    store.add_audit_event(
        "offer_parallel_fallback",
        &json!({
            "market_id": market_id,
            "error": err.to_string(),
            "reason": "reservation_parallel_path_failed",
        }),
        Some(market_id),
    )
}

pub async fn execute_strategy_actions(
    store: &SqliteStore,
    ctx: &MarketCycleContext<'_>,
    market: &MarketConfig,
    actions: &[PlannedAction],
) -> SignerResult<OfferDispatchOutput> {
    if !ctx.resources.program.signer_offer_path_configured() {
        store.add_audit_event(
            "strategy_exec_skipped_no_signer",
            &json!({"market_id": market.market_id, "planned_count": actions.len()}),
            Some(&market.market_id),
        )?;
        return Ok(OfferDispatchOutput {
            executed_count: 0,
            newly_executed_sell_counts: BTreeMap::new(),
        });
    }

    let expanded = expand_planned_actions(actions);
    if expanded.is_empty() {
        return Ok(OfferDispatchOutput {
            executed_count: 0,
            newly_executed_sell_counts: BTreeMap::new(),
        });
    }

    let program = &ctx.resources.program;
    if parallel_managed_dispatch_enabled(program) {
        match classify_parallel_dispatch(
            parallel::execute_actions_parallel(
                store,
                &ctx.dispatch.db_path,
                ctx.resources,
                market,
                &expanded,
            )
            .await,
        ) {
            ParallelDispatchDecision::Success(output) => return Ok(output),
            ParallelDispatchDecision::FallbackTransient(err) => {
                record_parallel_fallback_audit(store, &market.market_id, &err).await?;
            }
            ParallelDispatchDecision::Fatal(err) => return Err(err),
        }
    }

    sequential::execute_actions_sequential(program, &ctx.resources.paths, market, &expanded).await
}
