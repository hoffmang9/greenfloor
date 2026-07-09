use std::future::Future;
use std::pin::Pin;

use crate::config::MarketConfig;
use crate::cycle::MarketCycleResultState;
use crate::error::{SignerError, SignerResult};
use crate::locked_logged_phase;
use crate::operator_log::{LogContext, MARKET_CYCLE_COMPLETED, MARKET_CYCLE_STARTED};
use crate::storage::CycleWriteStore;

use super::cancel_phase::run_market_cancel_phase;
use super::coin_ops_phase::run_coin_ops_phase;
use super::cycle_store::run_logged_market_phase;
use super::inventory_phase::run_inventory_phase;
use super::market_context::MarketCycleContext;
use super::market_gate::enforce_market_key_allowlist;
use super::strategy_phase::run_strategy_phase;

pub fn run_post_reconcile_market_phases<'a>(
    write_store: &'a CycleWriteStore,
    ctx: &'a MarketCycleContext<'a>,
    market: &'a MarketConfig,
) -> Pin<Box<dyn Future<Output = SignerResult<MarketCycleResultState>> + 'a>> {
    Box::pin(run_post_reconcile_market_phases_async(
        write_store,
        ctx,
        market,
    ))
}

#[allow(clippy::large_futures)] // sequential phase orchestration stacks market context futures
async fn execute_post_reconcile_phases(
    write_store: &CycleWriteStore,
    ctx: &MarketCycleContext<'_>,
    market: &MarketConfig,
    cycle_state: &mut MarketCycleResultState,
) -> SignerResult<()> {
    let bucket_counts = locked_logged_phase!(
        market.market_id.as_str(),
        "inventory",
        write_store,
        |store| { run_inventory_phase(&store, ctx.resources, market, cycle_state) }
    )
    .await?;

    let strategy = run_logged_market_phase(
        market.market_id.as_str(),
        "strategy",
        run_strategy_phase(write_store, ctx, market, cycle_state),
    )
    .await?;

    locked_logged_phase!(market.market_id.as_str(), "cancel", write_store, |store| {
        run_market_cancel_phase(&store, ctx, market, cycle_state)
    })
    .await?;

    locked_logged_phase!(
        market.market_id.as_str(),
        "coin_ops",
        write_store,
        |store| {
            run_coin_ops_phase(
                &store,
                ctx,
                market,
                &bucket_counts,
                &strategy.sell_active_counts,
                &strategy.newly_executed_sell_counts,
            )
        }
    )
    .await?;
    Ok(())
}

async fn run_post_reconcile_market_phases_async(
    write_store: &CycleWriteStore,
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
