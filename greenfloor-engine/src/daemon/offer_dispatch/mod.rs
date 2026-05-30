//! Managed offer dispatch for the daemon strategy phase (sequential and parallel).

mod coordinator;
mod parallel;
mod reservation_ctx;
mod sequential;
mod spendable;

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::json;

use crate::config::{require_signer_offer_path, ManagerProgramConfig, MarketConfig};
use crate::cycle::{
    can_parallelize_managed_offers, expand_planned_actions, is_parallel_dispatch_transient_error,
    PlannedAction,
};
use crate::error::{SignerError, SignerResult};
use crate::storage::SqliteStore;

pub use coordinator::OfferReservationCoordinator;

#[derive(Debug, Clone)]
pub struct OfferDispatchOutput {
    pub executed_count: u64,
    pub newly_executed_sell_counts: BTreeMap<i64, i64>,
}

pub fn skip_strategy_execution() -> bool {
    std::env::var_os("GREENFLOOR_TEST_SKIP_STRATEGY_EXEC").is_some()
}

fn parallel_transient_error(err: &SignerError) -> bool {
    let message = err.to_string();
    is_parallel_dispatch_transient_error("ManagedUpstreamTransientError", &message)
        || is_parallel_dispatch_transient_error("ReservationContentionError", &message)
        || message.contains("database is locked")
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

    let use_parallel = can_parallelize_managed_offers(
        true,
        program.runtime_offer_parallelism_enabled,
        program.runtime_dry_run,
        true,
    );

    if use_parallel {
        match parallel::execute_actions_parallel(
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
        .await
        {
            Ok(output) => return Ok(output),
            Err(err) if parallel_transient_error(&err) => {
                store.add_audit_event(
                    "offer_parallel_fallback",
                    &json!({
                        "market_id": market.market_id,
                        "error": err.to_string(),
                        "reason": "reservation_parallel_path_failed",
                    }),
                    Some(&market.market_id),
                )?;
            }
            Err(err) => return Err(err),
        }
    }

    sequential::execute_actions_sequential(
        store,
        program,
        market,
        program_path,
        markets_path,
        testnet_markets_path,
        actions,
    )
    .await
}
