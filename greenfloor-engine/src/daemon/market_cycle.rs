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

pub struct PostReconcilePhaseOutput {
    pub state: MarketCycleResultState,
}

pub async fn run_post_reconcile_market_phases(
    store: &SqliteStore,
    ctx: &MarketCycleContext<'_>,
    market: &MarketConfig,
) -> SignerResult<PostReconcilePhaseOutput> {
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

    let mut state = MarketCycleResultState::default();

    let bucket_counts = run_inventory_phase(
        store,
        &ctx.resources.program_path,
        &ctx.resources.program,
        market,
        &ctx.resources.network,
        &mut state,
    )
    .await?;

    let strategy = run_strategy_phase(
        store,
        &ctx.dispatch.db_path,
        &ctx.resources.program,
        market,
        &ctx.resources.network,
        &ctx.resources.program_path,
        &ctx.resources.markets_path,
        ctx.resources.testnet_markets_path.as_deref(),
        ctx.reconcile,
        ctx.dispatch.xch_price_usd,
        ctx.dispatch.test_controls.skip_strategy_execution,
        &mut state,
    )
    .await?;

    let _cancel_payload = run_market_cancel_phase(
        store,
        &ctx.resources.dexie,
        market,
        &ctx.reconcile.offers,
        ctx.dispatch.runtime_dry_run,
        ctx.dispatch.xch_price_usd,
        ctx.plan.previous_xch_price_usd,
        &mut state,
    )
    .await?;

    run_coin_ops_phase(
        store,
        market,
        &ctx.resources.program,
        &ctx.resources.program_path,
        &ctx.reconcile.offers,
        &bucket_counts,
        &strategy.sell_active_counts,
        &strategy.newly_executed_sell_counts,
    )
    .await?;

    Ok(PostReconcilePhaseOutput { state })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cycle::{post_reconcile_market_cycle_phases, MarketCyclePhase};

    #[test]
    fn post_reconcile_phases_follow_canonical_order() {
        assert_eq!(
            post_reconcile_market_cycle_phases(),
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
