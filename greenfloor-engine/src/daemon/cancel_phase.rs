use serde_json::{json, Value};
use tracing::Level;

use crate::adapters::DexieClient;
use crate::config::{
    cancel_policy_stable_vs_unstable, is_signer_execution_soft_skip, signer_execution_skip_reason,
    MarketConfig,
};
use crate::cycle::{
    evaluate_cancel_policy_decision, CancelPolicyDecision, MarketCycleResultState, ReconcileState,
};
use crate::error::SignerResult;
use crate::offer::lifecycle::{
    cancel_offers_on_chain, cancel_targets_need_dexie_fallback,
    collect_market_cancel_target_offer_ids, defer_in_flight_cancel_offer_ids, CancelOfferTarget,
};
use crate::operator_log::{LogContext, OFFER_CANCEL_POLICY};
use crate::storage::SqliteStore;
use chrono::Utc;
use std::collections::HashMap;

use super::market_context::MarketCycleContext;

fn cancel_policy_payload(
    market_id: &str,
    decision: &CancelPolicyDecision,
    cancel_planned: i64,
    cancel_executed: i64,
    items: &[Value],
) -> Value {
    json!({
        "market_id": market_id,
        "eligible": decision.eligible,
        "triggered": decision.triggered,
        "reason": decision.reason,
        "move_bps": decision.move_bps,
        "threshold_bps": decision.threshold_bps,
        "planned_count": cancel_planned,
        "executed_count": cancel_executed,
        "items": items,
    })
}

fn audit_cancel_policy(
    store: &SqliteStore,
    market_id: &str,
    decision: &CancelPolicyDecision,
    cancel_planned: i64,
    cancel_executed: i64,
    items: &[Value],
) -> SignerResult<Value> {
    let payload =
        cancel_policy_payload(market_id, decision, cancel_planned, cancel_executed, items);
    LogContext::MARKET_CYCLE.dual_audit(
        store,
        Level::INFO,
        "offer cancel policy evaluated",
        OFFER_CANCEL_POLICY,
        &payload,
        Some(market_id),
    )?;
    Ok(payload)
}

async fn execute_on_chain_cancellations(
    store: &SqliteStore,
    dexie: Option<&DexieClient>,
    signer_config: crate::config::SignerConfig,
    operator_network: &str,
    market: &MarketConfig,
    target_offer_ids: &[String],
) -> SignerResult<(i64, Vec<Value>)> {
    let targets: Vec<CancelOfferTarget> = target_offer_ids
        .iter()
        .map(|offer_id| CancelOfferTarget::Tracked {
            offer_id: offer_id.clone(),
            market_id: market.market_id.clone(),
        })
        .collect();
    let dexie = if cancel_targets_need_dexie_fallback(store, &targets)? {
        dexie
    } else {
        None
    };
    let outcomes =
        cancel_offers_on_chain(store, dexie, signer_config, operator_network, &targets).await?;
    let mut cancel_executed = 0_i64;
    let mut items = Vec::with_capacity(outcomes.len());
    for outcome in outcomes {
        if outcome.success {
            cancel_executed += 1;
            let mut item = json!({
                "offer_id": outcome.offer_id,
                "status": ReconcileState::CancelSubmitted.as_str(),
                "reason": "cancel_submitted_on_strong_unstable_move",
                "operation_id": outcome.operation_id,
                "attempts": 1,
            });
            if !outcome.warning.is_empty() {
                if let Some(obj) = item.as_object_mut() {
                    obj.insert("warning".to_string(), json!(outcome.warning));
                }
            }
            items.push(item);
        } else {
            let error = if outcome.error.is_empty() {
                "cancel_failed"
            } else {
                outcome.error.as_str()
            };
            items.push(json!({
                "offer_id": outcome.offer_id,
                "status": "skipped",
                "reason": format!("cancel_failed:{error}"),
                "attempts": 1,
            }));
        }
    }
    Ok((cancel_executed, items))
}

fn cancel_target_offer_ids(
    store: &SqliteStore,
    market_id: &str,
    dexie_status_by_lookup_key: &HashMap<String, i64>,
) -> SignerResult<Vec<String>> {
    let target_offer_ids =
        collect_market_cancel_target_offer_ids(store, market_id, dexie_status_by_lookup_key)?;
    if target_offer_ids.is_empty() {
        return Ok(target_offer_ids);
    }
    let db_rows = store.list_offer_states_for_ids(&target_offer_ids)?;
    defer_in_flight_cancel_offer_ids(store, &db_rows, &target_offer_ids, Utc::now())
}

fn evaluate_market_cancel_decision(
    market: &MarketConfig,
    current_xch_price_usd: Option<f64>,
    previous_xch_price_usd: Option<f64>,
) -> CancelPolicyDecision {
    let env_threshold = std::env::var("GREENFLOOR_UNSTABLE_CANCEL_MOVE_BPS")
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .filter(|value| *value > 0);
    evaluate_cancel_policy_decision(
        &market.quote_asset_type,
        cancel_policy_stable_vs_unstable(&market.pricing),
        current_xch_price_usd,
        previous_xch_price_usd,
        market.cancel_move_threshold_bps,
        env_threshold,
    )
}

fn dry_run_cancel_items(target_offer_ids: &[String]) -> Vec<Value> {
    target_offer_ids
        .iter()
        .map(|offer_id| {
            json!({
                "offer_id": offer_id,
                "status": "planned",
                "reason": "dry_run",
            })
        })
        .collect()
}

fn soft_skip_cancel_items(target_offer_ids: &[String], reason: &str) -> Vec<Value> {
    target_offer_ids
        .iter()
        .map(|offer_id| {
            json!({
                "offer_id": offer_id,
                "status": "skipped",
                "reason": format!("cancel_skipped:{reason}"),
                "attempts": 0,
            })
        })
        .collect()
}

/// Run market cancel phase.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub async fn run_market_cancel_phase(
    store: &SqliteStore,
    ctx: &MarketCycleContext<'_>,
    market: &MarketConfig,
    state: &mut MarketCycleResultState,
) -> SignerResult<Value> {
    let market_id = market.market_id.as_str();
    let decision = evaluate_market_cancel_decision(
        market,
        ctx.dispatch.xch_price_usd,
        ctx.plan.previous_xch_price_usd,
    );
    let target_offer_ids =
        cancel_target_offer_ids(store, market_id, &ctx.reconcile.dexie_status_by_lookup_key)?;
    let cancel_planned =
        crate::config::usize_to_i64(target_offer_ids.len(), "cancel.target_offer_ids.len")?;

    if !decision.triggered {
        let payload = audit_cancel_policy(store, market_id, &decision, cancel_planned, 0, &[])?;
        state.merge_cancel_policy(false, cancel_planned, 0);
        return Ok(payload);
    }

    let (cancel_executed, items) = if ctx.dispatch.runtime_dry_run {
        (0_i64, dry_run_cancel_items(&target_offer_ids))
    } else {
        match ctx.resources.signer_for_execution() {
            Err(err) if is_signer_execution_soft_skip(&err) => {
                let items =
                    soft_skip_cancel_items(&target_offer_ids, &signer_execution_skip_reason(&err));
                let payload =
                    audit_cancel_policy(store, market_id, &decision, cancel_planned, 0, &items)?;
                state.merge_cancel_policy(true, cancel_planned, 0);
                return Ok(payload);
            }
            Err(err) => return Err(err),
            Ok(signer) => {
                execute_on_chain_cancellations(
                    store,
                    Some(&ctx.resources.dexie),
                    signer.clone(),
                    &ctx.resources.network,
                    market,
                    &target_offer_ids,
                )
                .await?
            }
        }
    };

    let payload = audit_cancel_policy(
        store,
        market_id,
        &decision,
        cancel_planned,
        cancel_executed,
        &items,
    )?;
    state.merge_cancel_policy(true, cancel_planned, cancel_executed);
    Ok(payload)
}
