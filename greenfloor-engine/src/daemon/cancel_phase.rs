use serde_json::{json, Value};

use crate::config::{cancel_policy_stable_vs_unstable, MarketConfig};
use crate::cycle::{
    collect_open_offer_ids_for_cancel, evaluate_cancel_policy_decision, MarketCycleResultState,
};
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use crate::offer::dexie_payload::dexie_offer_status;
use crate::offer::lifecycle::{cancel_offers_on_dexie, CancelOfferTarget};

use super::market_context::MarketCycleContext;

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

    let offer_rows: Vec<(String, i64)> = offers
        .iter()
        .filter_map(|offer| {
            let offer_id = offer.as_object()?.get("id")?.as_str()?.trim().to_string();
            if offer_id.is_empty() {
                return None;
            }
            Some((offer_id, dexie_offer_status(offer).unwrap_or(-1)))
        })
        .collect();
    let target_offer_ids = collect_open_offer_ids_for_cancel(&offer_rows);
    let mut items = Vec::new();
    let cancel_planned = target_offer_ids.len() as i64;
    let mut cancel_executed = 0_i64;

    if !decision.triggered {
        let payload = json!({
            "market_id": market_id,
            "eligible": decision.eligible,
            "triggered": decision.triggered,
            "reason": decision.reason,
            "move_bps": decision.move_bps,
            "threshold_bps": decision.threshold_bps,
            "planned_count": cancel_planned,
            "executed_count": 0,
            "items": items,
        });
        store.add_audit_event("offer_cancel_policy", &payload, Some(market_id))?;
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
        let targets: Vec<CancelOfferTarget> = target_offer_ids
            .iter()
            .map(|offer_id| CancelOfferTarget {
                offer_id: offer_id.clone(),
                market_id: market_id.to_string(),
            })
            .collect();
        let outcomes = cancel_offers_on_dexie(store, dexie, &targets).await?;
        for outcome in outcomes {
            if outcome.success {
                cancel_executed += 1;
                items.push(json!({
                    "offer_id": outcome.offer_id,
                    "status": "executed",
                    "reason": "cancelled_on_strong_unstable_move",
                    "attempts": 1,
                }));
            } else {
                let error = if outcome.error.is_empty() {
                    outcome
                        .venue_response
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("cancel_failed")
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
    }

    let payload = json!({
        "market_id": market_id,
        "eligible": decision.eligible,
        "triggered": decision.triggered,
        "reason": decision.reason,
        "move_bps": decision.move_bps,
        "threshold_bps": decision.threshold_bps,
        "planned_count": cancel_planned,
        "executed_count": cancel_executed,
        "items": items,
    });
    store.add_audit_event("offer_cancel_policy", &payload, Some(market_id))?;
    state.merge_cancel_policy(cancel_triggered, cancel_planned, cancel_executed);
    Ok(payload)
}
