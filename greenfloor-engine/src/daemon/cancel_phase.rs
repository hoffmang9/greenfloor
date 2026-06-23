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
use crate::offer::dexie_payload::dexie_offer_status;
use crate::offer::lifecycle::{
    cancel_offers_on_chain, collect_dexie_open_offer_ids, defer_in_flight_cancel_offer_ids,
    CancelOfferTarget,
};
use crate::operator_log::{LogContext, OFFER_CANCEL_POLICY};
use crate::storage::SqliteStore;
use chrono::Utc;

use super::market_context::MarketCycleContext;

fn cancel_offer_status_rows(offers: &[Value]) -> Vec<(String, i64)> {
    offers
        .iter()
        .filter_map(|offer| {
            let offer_id = offer.as_object()?.get("id")?.as_str()?.trim().to_string();
            if offer_id.is_empty() {
                return None;
            }
            Some((offer_id, dexie_offer_status(offer).unwrap_or(-1)))
        })
        .collect()
}

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

async fn execute_on_chain_cancellations(
    store: &SqliteStore,
    dexie: &DexieClient,
    signer_config: crate::config::SignerConfig,
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
    let outcomes = cancel_offers_on_chain(store, dexie, signer_config, &targets).await?;
    let mut cancel_executed = 0_i64;
    let mut items = Vec::with_capacity(outcomes.len());
    for outcome in outcomes {
        if outcome.success {
            cancel_executed += 1;
            items.push(json!({
                "offer_id": outcome.offer_id,
                "status": ReconcileState::CancelSubmitted.as_str(),
                "reason": "cancel_submitted_on_strong_unstable_move",
                "operation_id": outcome.operation_id,
                "attempts": 1,
            }));
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

fn cancel_target_offer_ids(store: &SqliteStore, offers: &[Value]) -> SignerResult<Vec<String>> {
    let offer_rows = cancel_offer_status_rows(offers);
    let target_offer_ids = collect_dexie_open_offer_ids(&offer_rows);
    if target_offer_ids.is_empty() {
        return Ok(target_offer_ids);
    }
    let db_rows = store.list_offer_states_for_ids(&target_offer_ids)?;
    defer_in_flight_cancel_offer_ids(store, &db_rows, &target_offer_ids, Utc::now())
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
    offers: &[Value],
    state: &mut MarketCycleResultState,
) -> SignerResult<Value> {
    let dexie = &ctx.resources.dexie;
    let runtime_dry_run = ctx.dispatch.runtime_dry_run;
    let current_xch_price_usd = ctx.dispatch.xch_price_usd;
    let previous_xch_price_usd = ctx.plan.previous_xch_price_usd;
    let market_id = market.market_id.as_str();
    let env_threshold = std::env::var("GREENFLOOR_UNSTABLE_CANCEL_MOVE_BPS")
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .filter(|value| *value > 0);
    let decision = evaluate_cancel_policy_decision(
        &market.quote_asset_type,
        cancel_policy_stable_vs_unstable(&market.pricing),
        current_xch_price_usd,
        previous_xch_price_usd,
        market.cancel_move_threshold_bps,
        env_threshold,
    );

    let target_offer_ids = cancel_target_offer_ids(store, offers)?;
    let mut items = Vec::new();
    let cancel_planned =
        crate::config::usize_to_i64(target_offer_ids.len(), "cancel.target_offer_ids.len")?;
    let mut cancel_executed = 0_i64;

    if !decision.triggered {
        let payload = cancel_policy_payload(market_id, &decision, cancel_planned, 0, &items);
        LogContext::MARKET_CYCLE.dual_audit(
            store,
            Level::INFO,
            "offer cancel policy evaluated",
            OFFER_CANCEL_POLICY,
            &payload,
            Some(market_id),
        )?;
        state.merge_cancel_policy(false, cancel_planned, 0);
        return Ok(payload);
    }

    let cancel_triggered = true;
    if runtime_dry_run {
        for offer_id in &target_offer_ids {
            items.push(json!({
                "offer_id": offer_id,
                "status": "planned",
                "reason": "dry_run",
            }));
        }
    } else {
        let signer_config = match ctx.resources.signer_for_execution() {
            Err(err) if is_signer_execution_soft_skip(&err) => {
                for offer_id in &target_offer_ids {
                    items.push(json!({
                        "offer_id": offer_id,
                        "status": "skipped",
                        "reason": format!("cancel_skipped:{}", signer_execution_skip_reason(&err)),
                        "attempts": 0,
                    }));
                }
                let payload =
                    cancel_policy_payload(market_id, &decision, cancel_planned, 0, &items);
                LogContext::MARKET_CYCLE.dual_audit(
                    store,
                    Level::INFO,
                    "offer cancel policy evaluated",
                    OFFER_CANCEL_POLICY,
                    &payload,
                    Some(market_id),
                )?;
                state.merge_cancel_policy(cancel_triggered, cancel_planned, 0);
                return Ok(payload);
            }
            Err(err) => return Err(err),
            Ok(signer) => signer.clone(),
        };
        (cancel_executed, items) =
            execute_on_chain_cancellations(store, dexie, signer_config, market, &target_offer_ids)
                .await?;
    }

    let payload = cancel_policy_payload(
        market_id,
        &decision,
        cancel_planned,
        cancel_executed,
        &items,
    );
    LogContext::MARKET_CYCLE.dual_audit(
        store,
        Level::INFO,
        "offer cancel policy evaluated",
        OFFER_CANCEL_POLICY,
        &payload,
        Some(market_id),
    )?;
    state.merge_cancel_policy(cancel_triggered, cancel_planned, cancel_executed);
    Ok(payload)
}
