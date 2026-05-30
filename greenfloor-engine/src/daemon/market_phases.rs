use std::collections::BTreeMap;
use std::path::Path;

use serde_json::json;

use crate::coin_ops::{compute_bucket_counts_from_coins, plan_coin_ops, BucketSpec};
use crate::coinset::list_wallet_unspent_coins;
use crate::config::{action_side_from_pricing, require_signer_offer_path, MarketConfig,
    ManagerProgramConfig};
use crate::cycle::PlannedAction;
use crate::error::{SignerError, SignerResult};
use crate::manager::{build_and_post_offer, BuildAndPostOfferRequest};
use crate::storage::SqliteStore;

use super::reconcile_phase::ReconcilePhaseResult;
use super::strategy_support::{active_offer_counts_by_size, evaluate_strategy_actions};

#[derive(Debug, Clone, Default)]
pub struct MarketPhaseMetrics {
    pub cycle_error_count: u64,
    pub strategy_planned_total: u64,
    pub strategy_executed_total: u64,
}

pub async fn run_market_phases(
    store: &SqliteStore,
    program: &ManagerProgramConfig,
    market: &MarketConfig,
    network: &str,
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
    reconcile: &ReconcilePhaseResult,
    xch_price_usd: Option<f64>,
) -> SignerResult<MarketPhaseMetrics> {
    if test_forced_market_error(&market.market_id) {
        return Err(SignerError::Other(format!(
            "forced market error for {}",
            market.market_id
        )));
    }

    let mut metrics = MarketPhaseMetrics::default();
    let bucket_counts = scan_inventory_buckets(store, program, market, network, &mut metrics).await?;
    let active_counts = active_offer_counts_by_size(&reconcile.offers, &reconcile.dexie_size_by_offer_id);
    let strategy_actions =
        evaluate_strategy_actions(market, network, &active_counts, xch_price_usd);
    metrics.strategy_planned_total = strategy_actions.len() as u64;

    store.add_audit_event(
        "strategy_actions_planned",
        &json!({
            "market_id": market.market_id,
            "xch_price_usd": xch_price_usd,
            "action_count": strategy_actions.len(),
        }),
        Some(&market.market_id),
    )?;

    if !strategy_actions.is_empty() && !skip_strategy_execution() {
        match execute_strategy_actions(
            store,
            program,
            market,
            program_path,
            markets_path,
            testnet_markets_path,
            &strategy_actions,
        )
        .await
        {
            Ok(executed) => {
                metrics.strategy_executed_total = executed;
                store.add_audit_event(
                    "strategy_offer_execution",
                    &json!({
                        "market_id": market.market_id,
                        "planned_count": strategy_actions.len(),
                        "executed_count": executed,
                    }),
                    Some(&market.market_id),
                )?;
            }
            Err(err) => {
                metrics.cycle_error_count += 1;
                store.add_audit_event(
                    "strategy_offer_execution_error",
                    &json!({"market_id": market.market_id, "error": err.to_string()}),
                    Some(&market.market_id),
                )?;
            }
        }
    }

    plan_coin_ops_phase(store, market, program, &bucket_counts, &active_counts, &mut metrics)?;
    Ok(metrics)
}

async fn scan_inventory_buckets(
    store: &SqliteStore,
    _program: &ManagerProgramConfig,
    market: &MarketConfig,
    network: &str,
    metrics: &mut MarketPhaseMetrics,
) -> SignerResult<BTreeMap<i64, i64>> {
    let ladder_sizes: Vec<i64> = market
        .ladders
        .get("sell")
        .into_iter()
        .flat_map(|entries| entries.iter().map(|entry| entry.size_base_units))
        .filter(|size| *size > 0)
        .collect();
    if ladder_sizes.is_empty() {
        return Ok(BTreeMap::new());
    }

    let resolved_asset = market.base_asset.trim().to_string();

    match list_wallet_unspent_coins(network, &market.receive_address, &resolved_asset).await {
        Ok(coins) => {
            let amounts: Vec<i64> = coins
                .into_iter()
                .map(|coin| i64::try_from(coin.amount).unwrap_or(i64::MAX))
                .collect();
            let bucket_counts = compute_bucket_counts_from_coins(&amounts, &ladder_sizes);
            store.add_audit_event(
                "inventory_bucket_scan",
                &json!({
                    "market_id": market.market_id,
                    "source": "coinset",
                    "coin_count": amounts.len(),
                    "bucket_counts": bucket_counts,
                }),
                Some(&market.market_id),
            )?;
            Ok(bucket_counts)
        }
        Err(err) => {
            metrics.cycle_error_count += 1;
            store.add_audit_event(
                "inventory_bucket_scan_error",
                &json!({"market_id": market.market_id, "error": err.to_string()}),
                Some(&market.market_id),
            )?;
            Ok(BTreeMap::new())
        }
    }
}

async fn execute_strategy_actions(
    store: &SqliteStore,
    program: &ManagerProgramConfig,
    market: &MarketConfig,
    program_path: &Path,
    markets_path: &Path,
    testnet_markets_path: Option<&Path>,
    actions: &[PlannedAction],
) -> SignerResult<u64> {
    if require_signer_offer_path(program_path).is_err() {
        store.add_audit_event(
            "strategy_exec_skipped_no_signer",
            &json!({"market_id": market.market_id, "planned_count": actions.len()}),
            Some(&market.market_id),
        )?;
        return Ok(0);
    }

    let mut executed = 0_u64;
    for action in actions {
        if action.repeat <= 0 || action.size <= 0 {
            continue;
        }
        let response = build_and_post_offer(BuildAndPostOfferRequest {
            program_path: program_path.to_path_buf(),
            markets_path: markets_path.to_path_buf(),
            testnet_markets_path: testnet_markets_path.map(Path::to_path_buf),
            network: program.network.clone(),
            market_id: Some(market.market_id.clone()),
            pair: None,
            size_base_units: action.size as u64,
            repeat: action.repeat as u32,
            publish_venue: Some(program.offer_publish_venue.clone()),
            dexie_base_url: Some(program.dexie_api_base.clone()),
            splash_base_url: Some(program.splash_api_base.clone()),
            drop_only: true,
            claim_rewards: false,
            dry_run: program.runtime_dry_run,
            compact_json: false,
            persist_results: true,
            action_side: Some(action_side_from_pricing(&market.pricing)),
        })
        .await?;
        if response.exit_code == 0 {
            executed += action.repeat as u64;
        }
    }
    Ok(executed)
}

fn plan_coin_ops_phase(
    store: &SqliteStore,
    market: &MarketConfig,
    program: &ManagerProgramConfig,
    bucket_counts: &BTreeMap<i64, i64>,
    active_counts: &BTreeMap<i64, i64>,
    metrics: &mut MarketPhaseMetrics,
) -> SignerResult<()> {
    let sell_ladder = market.ladders.get("sell").cloned().unwrap_or_default();
    if sell_ladder.is_empty() {
        store.add_audit_event(
            "coin_ops_no_plans",
            &json!({"market_id": market.market_id, "reason": "empty_sell_ladder"}),
            Some(&market.market_id),
        )?;
        return Ok(());
    }
    let buckets: Vec<BucketSpec> = sell_ladder
        .into_iter()
        .map(|entry| BucketSpec {
            size_base_units: entry.size_base_units,
            target_count: entry.target_count,
            split_buffer_count: entry.split_buffer_count,
            combine_when_excess_factor: 2.0,
            current_count: bucket_counts
                .get(&entry.size_base_units)
                .copied()
                .unwrap_or(0)
                .max(*active_counts.get(&entry.size_base_units).unwrap_or(&0)),
        })
        .collect();
    let plans = plan_coin_ops(&buckets, 0, 0, 0, 0);
    store.add_audit_event(
        if plans.is_empty() {
            "coin_ops_no_plans"
        } else {
            "coin_ops_planned"
        },
        &json!({
            "market_id": market.market_id,
            "planned_count": plans.len(),
            "dry_run": program.runtime_dry_run,
        }),
        Some(&market.market_id),
    )?;
    if plans.is_empty() {
        return Ok(());
    }
    // Coin-op broadcast remains on the Rust execution backlog; planning stays canonical here.
    let _ = metrics;
    Ok(())
}

fn skip_strategy_execution() -> bool {
    std::env::var_os("GREENFLOOR_TEST_SKIP_STRATEGY_EXEC").is_some()
}

fn test_forced_market_error(market_id: &str) -> bool {
    std::env::var("GREENFLOOR_TEST_FORCE_MARKET_ERROR")
        .map(|value| value.trim() == market_id)
        .unwrap_or(false)
}
