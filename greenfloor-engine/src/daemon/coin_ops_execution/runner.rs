use std::collections::HashSet;

use serde_json::{json, Value};

use crate::async_boundary::ManagedCoinOpPlansFuture;
use crate::coin_ops::CoinOpPlan;
use crate::config::{ManagerProgramConfig, MarketConfig};
use crate::error::SignerResult;
use crate::operator_log::{coin_op_ledger_event, LogContext};
use crate::storage::SqliteStore;

use super::super::watchlist::watchlist_offer_ids;
use super::combine::execute_daemon_combine_plan;
use super::items::{skip_item, CoinOpExecItem, CoinOpExecutionResult};
use super::split::execute_daemon_split_plan;
use crate::coin_ops::execution::CoinOpExecContext;
#[cfg(test)]
use crate::coin_ops::execution::CoinOpTestOverrides;
use crate::config::GatedOperatorMarket;
use crate::offer::dexie_payload::extract_coin_ids_from_offer_payload;
use crate::offer::dexie_payload::DexieOfferPayload;

/// Watched coin ids from open offers.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn watched_coin_ids_from_open_offers(
    store: &SqliteStore,
    market_id: &str,
    offers: &[Value],
) -> SignerResult<HashSet<String>> {
    let watch_offer_ids = watchlist_offer_ids(store, market_id)?;
    let mut watched = HashSet::default();
    for offer in offers {
        let payload = DexieOfferPayload::new(offer.clone());
        let Some(offer_id) = payload.id() else {
            continue;
        };
        if !watch_offer_ids.contains(&offer_id) {
            continue;
        }
        for coin_id in extract_coin_ids_from_offer_payload(payload.body()) {
            watched.insert(coin_id);
        }
    }
    Ok(watched)
}

fn skip_all_plans(
    program: &ManagerProgramConfig,
    market: &MarketConfig,
    operator_network: &str,
    plans: &[CoinOpPlan],
    reason: &str,
    status: &str,
) -> CoinOpExecutionResult {
    CoinOpExecutionResult {
        dry_run: program.runtime_dry_run,
        planned_count: plans.len(),
        executed_count: 0,
        status: status.to_string(),
        items: plans
            .iter()
            .map(|plan| {
                skip_item(
                    plan.op_type.as_str(),
                    plan.size_base_units,
                    plan.op_count,
                    reason,
                )
            })
            .collect(),
        signer_selection: json!({
            "selected_source": "signer_registry",
            "key_id": market.signer_key_id,
            "network": operator_network,
        }),
    }
}

#[must_use]
pub fn execute_managed_coin_op_plans<'a>(
    gated: GatedOperatorMarket,
    plans: &'a [CoinOpPlan],
    watched_coin_ids: &'a HashSet<String>,
) -> ManagedCoinOpPlansFuture<'a> {
    Box::pin(execute_managed_coin_op_plans_async(
        gated,
        plans,
        watched_coin_ids,
        #[cfg(test)]
        CoinOpTestOverrides::default(),
    ))
}

/// Run managed coin-op plans with injected wallet/split overrides (tests only).
#[cfg(test)]
#[must_use]
pub fn execute_managed_coin_op_plans_with_test_overrides<'a>(
    gated: GatedOperatorMarket,
    plans: &'a [CoinOpPlan],
    watched_coin_ids: &'a HashSet<String>,
    test_overrides: CoinOpTestOverrides,
) -> ManagedCoinOpPlansFuture<'a> {
    Box::pin(execute_managed_coin_op_plans_async(
        gated,
        plans,
        watched_coin_ids,
        test_overrides,
    ))
}

async fn execute_managed_coin_op_plans_async(
    gated: GatedOperatorMarket,
    plans: &[CoinOpPlan],
    watched_coin_ids: &HashSet<String>,
    #[cfg(test)] test_overrides: CoinOpTestOverrides,
) -> CoinOpExecutionResult {
    if gated.market_row.receive_address.trim().is_empty() {
        return skip_all_plans(
            &gated.program,
            &gated.market_row,
            &gated.operator_network,
            plans,
            "signer_coin_ops_missing_receive_address",
            "skipped",
        );
    }

    let program = gated.program.clone();
    let market = gated.market_row.clone();
    let operator_network = gated.operator_network.clone();
    let ctx = match CoinOpExecContext::from_gated_market(
        gated,
        None,
        watched_coin_ids.iter().cloned().collect(),
        #[cfg(test)]
        test_overrides,
    )
    .await
    {
        Ok(ctx) => ctx,
        Err(err) => {
            return skip_all_plans(
                &program,
                &market,
                &operator_network,
                plans,
                &err.to_string(),
                "skipped",
            );
        }
    };

    let mut items = Vec::new();
    let mut executed_count = 0_u64;
    for plan in plans {
        if plan.op_count <= 0 || plan.size_base_units <= 0 {
            items.push(skip_item(
                plan.op_type.as_str(),
                plan.size_base_units,
                plan.op_count,
                "invalid_plan",
            ));
            continue;
        }
        if ctx.gated.program.runtime_dry_run {
            items.push(CoinOpExecItem {
                op_type: plan.op_type.as_str().to_string(),
                size_base_units: plan.size_base_units,
                op_count: plan.op_count,
                status: "planned".to_string(),
                reason: "dry_run:signer".to_string(),
                operation_id: None,
            });
            continue;
        }
        let (plan_items, plan_executed) = match plan.op_type {
            crate::coin_ops::CoinOpKind::Split => {
                Box::pin(execute_daemon_split_plan(&ctx, plan)).await
            }
            crate::coin_ops::CoinOpKind::Combine => {
                Box::pin(execute_daemon_combine_plan(&ctx, plan)).await
            }
        };
        items.extend(plan_items);
        executed_count += plan_executed;
    }

    CoinOpExecutionResult {
        dry_run: ctx.gated.program.runtime_dry_run,
        planned_count: plans.len(),
        executed_count,
        status: "signer".to_string(),
        items,
        signer_selection: json!({
            "selected_source": "signer_registry",
            "key_id": ctx.gated.market_row.signer_key_id,
            "network": ctx.gated.operator_network,
        }),
    }
}

/// Persist coin op execution.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn persist_coin_op_execution(
    store: &SqliteStore,
    market: &MarketConfig,
    program: &ManagerProgramConfig,
    execution: &CoinOpExecutionResult,
) -> SignerResult<()> {
    for item in &execution.items {
        let fee_mojos = if item.status == "executed" {
            let per_op_fee = if item.op_type == "split" {
                program.coin_ops_split_fee_mojos
            } else {
                program.coin_ops_combine_fee_mojos
            };
            per_op_fee.saturating_mul(item.op_count)
        } else {
            0
        };
        let payload = json!({
            "market_id": market.market_id,
            "op_type": item.op_type,
            "size_base_units": item.size_base_units,
            "op_count": item.op_count,
            "reason": item.reason,
            "operation_id": item.operation_id,
            "fee_mojos": fee_mojos,
            "item_status": item.status,
        });
        let (event, level) = coin_op_ledger_event(item.status.as_str());
        LogContext::MARKET_CYCLE.dual_audit(
            store,
            level,
            "coin op ledger row",
            event,
            &payload,
            Some(&market.market_id),
        )?;
        store.add_coin_op_ledger_entry(&crate::storage::CoinOpLedgerEntry {
            market_id: &market.market_id,
            op_type: &item.op_type,
            op_count: item.op_count,
            fee_mojos,
            status: &item.status,
            reason: &item.reason,
            operation_id: item.operation_id.as_deref(),
        })?;
    }
    Ok(())
}
