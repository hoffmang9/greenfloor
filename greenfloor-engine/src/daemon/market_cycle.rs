use std::collections::BTreeMap;

use crate::config::MarketConfig;
use crate::cycle::post_reconcile_market_cycle_phases;
use crate::error::{SignerError, SignerResult};
use crate::storage::SqliteStore;

use super::cancel_phase::{run_market_cancel_phase, CancelPhaseMetrics};
use super::coin_ops_phase::run_coin_ops_phase;
use super::inventory_phase::run_inventory_phase;
use super::market_context::MarketCycleContext;
use super::market_gate::enforce_market_key_allowlist;
use super::market_phases::MarketPhaseMetrics;
use super::strategy_phase::run_strategy_phase;

pub struct PostReconcilePhaseOutput {
    pub metrics: MarketPhaseMetrics,
    pub cancel: CancelPhaseMetrics,
}

pub async fn run_post_reconcile_market_phases(
    store: &SqliteStore,
    ctx: &MarketCycleContext<'_>,
    market: &MarketConfig,
) -> SignerResult<PostReconcilePhaseOutput> {
    if ctx
        .test_controls()
        .force_market_error_for
        .as_deref()
        .is_some_and(|forced| forced.trim() == market.market_id)
    {
        return Err(SignerError::Other(format!(
            "forced market error for {}",
            market.market_id
        )));
    }
    enforce_market_key_allowlist(market, ctx.allowed_key_ids())?;

    let mut metrics = MarketPhaseMetrics::default();
    let mut bucket_counts = BTreeMap::new();
    let mut sell_active_counts = BTreeMap::new();
    let mut newly_executed_sell_counts = BTreeMap::new();
    let mut cancel = CancelPhaseMetrics::default();

    for phase in post_reconcile_market_cycle_phases() {
        match phase {
            crate::cycle::MarketCyclePhase::Inventory => {
                bucket_counts = run_inventory_phase(
                    store,
                    ctx.program_path(),
                    ctx.program(),
                    market,
                    ctx.network(),
                    &mut metrics,
                )
                .await?;
            }
            crate::cycle::MarketCyclePhase::Strategy => {
                let strategy = run_strategy_phase(
                    store,
                    ctx.db_path(),
                    ctx.program(),
                    market,
                    ctx.network(),
                    ctx.program_path(),
                    ctx.markets_path(),
                    ctx.testnet_markets_path(),
                    ctx.reconcile,
                    ctx.xch_price_usd(),
                    ctx.skip_strategy_execution(),
                    &mut metrics,
                )
                .await?;
                sell_active_counts = strategy.sell_active_counts;
                newly_executed_sell_counts = strategy.newly_executed_sell_counts;
            }
            crate::cycle::MarketCyclePhase::Cancel => {
                let (cancel_metrics, _payload) = run_market_cancel_phase(
                    store,
                    ctx.dexie(),
                    market,
                    &ctx.reconcile.offers,
                    ctx.runtime_dry_run(),
                    ctx.xch_price_usd(),
                    ctx.previous_xch_price_usd(),
                )
                .await?;
                cancel = cancel_metrics;
            }
            crate::cycle::MarketCyclePhase::CoinOps => {
                run_coin_ops_phase(
                    store,
                    market,
                    ctx.program(),
                    ctx.program_path(),
                    &ctx.reconcile.offers,
                    &bucket_counts,
                    &sell_active_counts,
                    &newly_executed_sell_counts,
                )
                .await?;
            }
            crate::cycle::MarketCyclePhase::Reconcile => {}
        }
    }

    Ok(PostReconcilePhaseOutput { metrics, cancel })
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
}
