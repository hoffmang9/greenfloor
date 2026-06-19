use crate::config::MarketConfig;
use crate::cycle::MarketCycleResultState;
use crate::error::{SignerError, SignerResult};
use crate::storage::SqliteStore;

use super::cancel_phase::run_market_cancel_phase;
use super::coin_ops_phase::run_coin_ops_phase;
use super::inventory_phase::run_inventory_phase;
use super::market_context::MarketCycleContext;
use super::market_gate::enforce_market_key_allowlist;
use super::strategy_phase::run_strategy_phase;

pub async fn run_post_reconcile_market_phases(
    store: &SqliteStore,
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

    let mut cycle_state = MarketCycleResultState::default();

    let bucket_counts = run_inventory_phase(store, ctx.resources, market, &mut cycle_state).await?;

    let strategy = Box::pin(run_strategy_phase(store, ctx, market, &mut cycle_state)).await?;

    let _cancel_payload =
        run_market_cancel_phase(store, ctx, market, &ctx.reconcile.offers, &mut cycle_state)
            .await?;

    Box::pin(run_coin_ops_phase(
        store,
        ctx,
        market,
        &ctx.reconcile.offers,
        &bucket_counts,
        &strategy.sell_active_counts,
        &strategy.newly_executed_sell_counts,
    ))
    .await?;

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
