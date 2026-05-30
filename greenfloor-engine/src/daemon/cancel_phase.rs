use serde_json::{json, Value};

use crate::adapters::DexieClient;
use crate::config::{cancel_policy_stable_vs_unstable, MarketConfig};
use crate::cycle::{
    collect_open_offer_ids_for_cancel, evaluate_cancel_policy_decision,
};
use crate::error::SignerResult;
use crate::storage::SqliteStore;

use super::coinset_tx::dexie_offer_status;

#[derive(Debug, Clone, Default)]
pub struct CancelPhaseMetrics {
    pub cancel_triggered: bool,
    pub cancel_planned: u64,
    pub cancel_executed: u64,
}

pub async fn run_market_cancel_phase(
    store: &SqliteStore,
    dexie: &DexieClient,
    market: &MarketConfig,
    offers: &[Value],
    runtime_dry_run: bool,
    current_xch_price_usd: Option<f64>,
    previous_xch_price_usd: Option<f64>,
) -> SignerResult<(CancelPhaseMetrics, Value)> {
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
            let offer_id = offer
                .as_object()?
                .get("id")?
                .as_str()?
                .trim()
                .to_string();
            if offer_id.is_empty() {
                return None;
            }
            Some((offer_id, dexie_offer_status(offer).unwrap_or(-1)))
        })
        .collect();
    let target_offer_ids = collect_open_offer_ids_for_cancel(&offer_rows);
    let mut items = Vec::new();
    let mut metrics = CancelPhaseMetrics::default();
    metrics.cancel_planned = target_offer_ids.len() as u64;

    if !decision.triggered {
        let payload = json!({
            "market_id": market_id,
            "eligible": decision.eligible,
            "triggered": decision.triggered,
            "reason": decision.reason,
            "move_bps": decision.move_bps,
            "threshold_bps": decision.threshold_bps,
            "planned_count": metrics.cancel_planned,
            "executed_count": 0,
            "items": items,
        });
        store.add_audit_event("offer_cancel_policy", &payload, Some(market_id))?;
        return Ok((metrics, payload));
    }

    metrics.cancel_triggered = true;
    for offer_id in target_offer_ids {
        if runtime_dry_run {
            items.push(json!({
                "offer_id": offer_id,
                "status": "planned",
                "reason": "dry_run",
            }));
            continue;
        }
        let result = dexie.cancel_offer(&offer_id).await?;
        let success = result.get("success").and_then(Value::as_bool) == Some(true);
        if success {
            metrics.cancel_executed += 1;
            store.upsert_offer_state(&offer_id, market_id, "cancelled", Some(3))?;
            items.push(json!({
                "offer_id": offer_id,
                "status": "executed",
                "reason": "cancelled_on_strong_unstable_move",
                "attempts": 1,
            }));
        } else {
            let error = result
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("cancel_failed");
            items.push(json!({
                "offer_id": offer_id,
                "status": "skipped",
                "reason": format!("cancel_failed:{error}"),
                "attempts": 1,
            }));
        }
    }

    let payload = json!({
        "market_id": market_id,
        "eligible": decision.eligible,
        "triggered": decision.triggered,
        "reason": decision.reason,
        "move_bps": decision.move_bps,
        "threshold_bps": decision.threshold_bps,
        "planned_count": metrics.cancel_planned,
        "executed_count": metrics.cancel_executed,
        "items": items,
    });
    store.add_audit_event("offer_cancel_policy", &payload, Some(market_id))?;
    Ok((metrics, payload))
}
