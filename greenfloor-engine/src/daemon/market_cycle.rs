// Sequential phases hold the std mutex across short sqlite-bound awaits by design;
// strategy parallel dispatch runs only after the lock is released between phases.
#![allow(clippy::await_holding_lock)]

use std::future::Future;
use std::pin::Pin;

use crate::config::MarketConfig;
use crate::cycle::MarketCycleResultState;
use crate::error::{SignerError, SignerResult};
use crate::operator_log::{LogContext, MARKET_CYCLE_COMPLETED, MARKET_CYCLE_STARTED, MARKET_PHASE};
use crate::storage::{lock_sqlite_store, SharedSqliteStore};
use tracing::Level;

use super::cancel_phase::run_market_cancel_phase;
use super::coin_ops_phase::run_coin_ops_phase;
use super::inventory_phase::run_inventory_phase;
use super::market_context::MarketCycleContext;
use super::market_gate::enforce_market_key_allowlist;
use super::strategy_phase::run_strategy_phase;

#[derive(Clone, Copy)]
enum MarketPhaseTraceOutcome {
    Started,
    Failed,
    Completed,
}

impl MarketPhaseTraceOutcome {
    const fn label(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Failed => "failed",
            Self::Completed => "completed",
        }
    }
}

fn trace_market_phase(
    market_id: &str,
    market_phase: &str,
    outcome: MarketPhaseTraceOutcome,
    level: Level,
) {
    macro_rules! emit {
        ($msg:literal) => {
            crate::trace_event!(
                level = level,
                LogContext::MARKET_CYCLE,
                MARKET_PHASE,
                {
                    market_id = market_id,
                    market_phase = market_phase,
                    outcome = outcome.label(),
                };
                $msg
            );
        };
    }
    match outcome {
        MarketPhaseTraceOutcome::Started => {
            emit!("market phase started");
        }
        MarketPhaseTraceOutcome::Failed => {
            emit!("market phase failed");
        }
        MarketPhaseTraceOutcome::Completed => {
            emit!("market phase completed");
        }
    }
}

async fn run_logged_market_phase<F, T>(
    market_id: &str,
    market_phase: &str,
    body: F,
) -> SignerResult<T>
where
    F: Future<Output = SignerResult<T>>,
{
    trace_market_phase(
        market_id,
        market_phase,
        MarketPhaseTraceOutcome::Started,
        Level::DEBUG,
    );
    let result = body.await;
    if result.is_err() {
        trace_market_phase(
            market_id,
            market_phase,
            MarketPhaseTraceOutcome::Failed,
            Level::WARN,
        );
    } else {
        trace_market_phase(
            market_id,
            market_phase,
            MarketPhaseTraceOutcome::Completed,
            Level::DEBUG,
        );
    }
    result
}

pub fn run_post_reconcile_market_phases<'a>(
    write_store: &'a SharedSqliteStore,
    ctx: &'a MarketCycleContext<'a>,
    market: &'a MarketConfig,
) -> Pin<Box<dyn Future<Output = SignerResult<MarketCycleResultState>> + 'a>> {
    Box::pin(run_post_reconcile_market_phases_async(
        write_store,
        ctx,
        market,
    ))
}

async fn execute_post_reconcile_phases(
    write_store: &SharedSqliteStore,
    ctx: &MarketCycleContext<'_>,
    market: &MarketConfig,
    cycle_state: &mut MarketCycleResultState,
) -> SignerResult<()> {
    let bucket_counts = {
        let store = lock_sqlite_store(write_store)?;
        run_logged_market_phase(
            market.market_id.as_str(),
            "inventory",
            run_inventory_phase(&store, ctx.resources, market, cycle_state),
        )
        .await?
    };

    let strategy = run_logged_market_phase(
        market.market_id.as_str(),
        "strategy",
        run_strategy_phase(write_store, ctx, market, cycle_state),
    )
    .await?;

    {
        let store = lock_sqlite_store(write_store)?;
        Box::pin(run_logged_market_phase(
            market.market_id.as_str(),
            "cancel",
            run_market_cancel_phase(&store, ctx, market, &ctx.reconcile.offers, cycle_state),
        ))
        .await?;
    }

    {
        let store = lock_sqlite_store(write_store)?;
        run_logged_market_phase(
            market.market_id.as_str(),
            "coin_ops",
            run_coin_ops_phase(
                &store,
                ctx,
                market,
                &ctx.reconcile.offers,
                &bucket_counts,
                &strategy.sell_active_counts,
                &strategy.newly_executed_sell_counts,
            ),
        )
        .await?;
    }
    Ok(())
}

async fn run_post_reconcile_market_phases_async(
    write_store: &SharedSqliteStore,
    ctx: &MarketCycleContext<'_>,
    market: &MarketConfig,
) -> SignerResult<MarketCycleResultState> {
    if ctx
        .dispatch
        .test_controls
        .force_market_error_for
        .as_deref()
        .is_some_and(|forced| forced.trim() == market.market_id)
    {
        return Err(SignerError::Other(format!(
            "forced market error for {}",
            market.market_id
        )));
    }
    enforce_market_key_allowlist(market, &ctx.dispatch.allowed_key_ids)?;

    crate::trace_event!(
        DEBUG,
        LogContext::MARKET_CYCLE,
        MARKET_CYCLE_STARTED,
        {
            market_id = market.market_id.as_str(),
            outcome = "started",
        };
        "market cycle started"
    );

    let mut cycle_state = MarketCycleResultState::default();

    Box::pin(execute_post_reconcile_phases(
        write_store,
        ctx,
        market,
        &mut cycle_state,
    ))
    .await?;

    crate::trace_event!(
        DEBUG,
        LogContext::MARKET_CYCLE,
        MARKET_CYCLE_COMPLETED,
        {
            market_id = market.market_id.as_str(),
            outcome = if cycle_state.cycle_errors > 0 {
                "partial_failure"
            } else {
                "success"
            },
            cycle_errors = cycle_state.cycle_errors,
        };
        "market cycle completed"
    );

    Ok(cycle_state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cycle::MarketCyclePhase;

    #[test]
    fn post_reconcile_phases_follow_canonical_order() {
        assert_eq!(
            crate::cycle::post_reconcile_market_cycle_phases(),
            &[
                MarketCyclePhase::Inventory,
                MarketCyclePhase::Strategy,
                MarketCyclePhase::Cancel,
                MarketCyclePhase::CoinOps,
            ]
        );
    }

    #[test]
    fn empty_market_cycle_result_state_is_default() {
        let state = MarketCycleResultState::default();
        assert_eq!(state.cycle_errors, 0);
        assert_eq!(state.strategy_planned, 0);
        assert!(!state.cancel_triggered);
    }
}
