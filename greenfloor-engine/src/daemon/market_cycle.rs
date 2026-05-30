use std::collections::BTreeMap;
use std::path::Path;

use crate::adapters::DexieClient;
use crate::config::{ManagerProgramConfig, MarketConfig};
use crate::cycle::{market_cycle_phases, MarketCyclePhase};
use crate::error::{SignerError, SignerResult};
use crate::storage::SqliteStore;

use super::cancel_phase::{run_market_cancel_phase, CancelPhaseMetrics};
use super::coin_ops_phase::run_coin_ops_phase;
use super::inventory_phase::run_inventory_phase;
use super::market_gate::enforce_market_key_allowlist;
use super::market_phases::MarketPhaseMetrics;
use super::reconcile_phase::ReconcilePhaseResult;
use super::strategy_phase::run_strategy_phase;

pub struct PostReconcilePhaseOutput {
    pub metrics: MarketPhaseMetrics,
    pub cancel: CancelPhaseMetrics,
}

pub async fn run_post_reconcile_market_phases(
    store: &SqliteStore,
    dexie: &DexieClient,
    program: &ManagerProgramConfig,
    market: &MarketConfig,
    network: &str,
    allowed_key_ids: &[String],
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
    reconcile: &ReconcilePhaseResult,
    xch_price_usd: Option<f64>,
    previous_xch_price_usd: Option<f64>,
    runtime_dry_run: bool,
) -> SignerResult<PostReconcilePhaseOutput> {
    if test_forced_market_error(&market.market_id) {
        return Err(SignerError::Other(format!(
            "forced market error for {}",
            market.market_id
        )));
    }
    enforce_market_key_allowlist(market, allowed_key_ids)?;

    let mut metrics = MarketPhaseMetrics::default();
    let mut bucket_counts = BTreeMap::new();
    let mut sell_active_counts = BTreeMap::new();
    let mut newly_executed_sell_counts = BTreeMap::new();
    let mut cancel = CancelPhaseMetrics::default();

    for phase in market_cycle_phases() {
        match phase {
            MarketCyclePhase::Reconcile => {}
            MarketCyclePhase::Inventory => {
                bucket_counts = run_inventory_phase(
                    store,
                    program_path,
                    program,
                    market,
                    network,
                    &mut metrics,
                )
                .await?;
            }
            MarketCyclePhase::Strategy => {
                let strategy = run_strategy_phase(
                    store,
                    program,
                    market,
                    network,
                    program_path,
                    markets_path,
                    testnet_markets_path,
                    reconcile,
                    xch_price_usd,
                    &mut metrics,
                )
                .await?;
                sell_active_counts = strategy.sell_active_counts;
                newly_executed_sell_counts = strategy.newly_executed_sell_counts;
            }
            MarketCyclePhase::Cancel => {
                let (cancel_metrics, _payload) = run_market_cancel_phase(
                    store,
                    dexie,
                    market,
                    &reconcile.offers,
                    runtime_dry_run,
                    xch_price_usd,
                    previous_xch_price_usd,
                )
                .await?;
                cancel = cancel_metrics;
            }
            MarketCyclePhase::CoinOps => {
                run_coin_ops_phase(
                    store,
                    market,
                    program,
                    program_path,
                    &reconcile.offers,
                    &bucket_counts,
                    &sell_active_counts,
                    &newly_executed_sell_counts,
                )
                .await?;
            }
        }
    }

    Ok(PostReconcilePhaseOutput { metrics, cancel })
}

fn test_forced_market_error(market_id: &str) -> bool {
    std::env::var("GREENFLOOR_TEST_FORCE_MARKET_ERROR")
        .map(|value| value.trim() == market_id)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cycle::MarketCyclePhase;

    #[test]
    fn post_reconcile_phases_follow_canonical_order_after_reconcile() {
        let phases: Vec<MarketCyclePhase> = market_cycle_phases()
            .iter()
            .copied()
            .filter(|phase| *phase != MarketCyclePhase::Reconcile)
            .collect();
        assert_eq!(
            phases,
            vec![
                MarketCyclePhase::Inventory,
                MarketCyclePhase::Strategy,
                MarketCyclePhase::Cancel,
                MarketCyclePhase::CoinOps,
            ]
        );
    }
}
