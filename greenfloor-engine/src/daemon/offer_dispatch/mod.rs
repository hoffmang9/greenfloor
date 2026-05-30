//! Managed offer dispatch for the daemon strategy phase (sequential and parallel).

mod coordinator;
mod managed_post;
mod parallel;
mod reservation_ctx;
mod sequential;
mod spendable;
mod test_hooks;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::json;

use crate::config::{require_signer_offer_path, ManagerProgramConfig, MarketConfig};
use crate::cycle::{
    expand_planned_actions, is_parallel_dispatch_transient_error,
    is_transient_managed_upstream_error_text, parallel_managed_dispatch_enabled, PlannedAction,
};
use crate::error::{SignerError, SignerResult};
use crate::storage::SqliteStore;

pub use coordinator::OfferReservationCoordinator;

#[derive(Debug, Clone)]
pub struct OfferDispatchOutput {
    pub executed_count: u64,
    pub newly_executed_sell_counts: BTreeMap<i64, i64>,
}

fn exception_class_prefix(message: &str) -> &str {
    message.split(':').next().unwrap_or(message).trim()
}

pub(crate) fn is_parallel_dispatch_transient_signer_error(err: &SignerError) -> bool {
    let message = err.to_string();
    if message.contains("database is locked") {
        return true;
    }
    let class = exception_class_prefix(&message);
    is_parallel_dispatch_transient_error(class, &message)
        || is_transient_managed_upstream_error_text(&message)
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
    db_path: &Path,
    program: &ManagerProgramConfig,
    market: &MarketConfig,
    network: &str,
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
    actions: &[PlannedAction],
) -> SignerResult<OfferDispatchOutput> {
    if require_signer_offer_path(program_path).is_err() {
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

    if parallel_managed_dispatch_enabled(program) {
        match classify_parallel_dispatch(
            parallel::execute_actions_parallel(
                store,
                db_path,
                program,
                market,
                network,
                program_path,
                markets_path,
                testnet_markets_path,
                actions,
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

    sequential::execute_actions_sequential(
        program,
        market,
        program_path,
        markets_path,
        testnet_markets_path,
        actions,
    )
    .await
}
