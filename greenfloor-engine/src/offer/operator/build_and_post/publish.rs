use std::path::Path;

use serde_json::{json, Value};
use tracing::Level;

use crate::adapters::{
    dexie_offer_view_url, post_offer_phase_dexie, DexieClient, PostOfferPhaseDexieParams,
    SplashClient,
};
use crate::error::{SignerError, SignerResult};
use crate::operator_log::{
    audit_and_trace, audit_market_cycle, trace_audit_outcome, LogContext, OFFER_POST_FAILURE,
    STRATEGY_OFFER_EXECUTION,
};
use crate::storage::{
    state_db_path_for_home, upsert_offer_post_record, OfferPostPersistRecord, SqliteStore,
};

use super::context::ResolvedBuildAndPostContext;
use super::types::PublishResult;
use super::BuildAndPostOfferRequest;

pub(super) struct PublishOfferParams<'a> {
    pub publish_venue: &'a str,
    pub dexie: Option<&'a DexieClient>,
    pub splash: Option<&'a SplashClient>,
    pub offer_text: &'a str,
    pub drop_only: bool,
    pub claim_rewards: bool,
    pub expected_offered_asset_id: &'a str,
    pub expected_offered_symbol: &'a str,
    pub expected_requested_asset_id: &'a str,
    pub expected_requested_symbol: &'a str,
}

pub(super) async fn publish_offer(params: PublishOfferParams<'_>) -> SignerResult<PublishResult> {
    let PublishOfferParams {
        publish_venue,
        dexie,
        splash,
        offer_text,
        drop_only,
        claim_rewards,
        expected_offered_asset_id,
        expected_offered_symbol,
        expected_requested_asset_id,
        expected_requested_symbol,
    } = params;
    let body = match publish_venue {
        "dexie" => {
            let dexie = dexie.ok_or_else(|| {
                SignerError::Other("dexie adapter missing for dexie publish".to_string())
            })?;
            post_offer_phase_dexie(PostOfferPhaseDexieParams {
                dexie,
                offer_text,
                drop_only,
                claim_rewards,
                expected_offered_asset_id,
                expected_offered_symbol,
                expected_requested_asset_id,
                expected_requested_symbol,
            })
            .await?
        }
        "splash" => {
            let splash = splash.ok_or_else(|| {
                SignerError::Other("splash adapter missing for splash publish".to_string())
            })?;
            splash.post_offer(offer_text).await?
        }
        other => {
            return Err(SignerError::Other(format!(
                "unsupported publish venue: {other}"
            )));
        }
    };
    Ok(PublishResult::from_adapter_body(body))
}

pub(super) fn finalize_publish_payload(
    publish: PublishResult,
    execution_mode: &str,
    timing_ms: Value,
    dexie_base_url: Option<&str>,
) -> Value {
    let mut payload = publish.body;
    if let Value::Object(obj) = &mut payload {
        obj.insert("execution_mode".to_string(), json!(execution_mode));
        obj.insert("timing_ms".to_string(), timing_ms);
        if publish.success {
            if let (Some(base_url), Some(offer_id)) = (dexie_base_url, publish.offer_id.as_deref())
            {
                obj.insert(
                    "offer_view_url".to_string(),
                    Value::String(dexie_offer_view_url(base_url, offer_id)),
                );
            }
        }
    }
    payload
}

pub(super) fn offer_post_persist_record(
    publish: &PublishResult,
    side: &str,
    execution_mode: &str,
    ctx: &ResolvedBuildAndPostContext,
    size_base_units: u64,
) -> Option<OfferPostPersistRecord> {
    if !publish.success {
        return None;
    }
    let offer_id = publish.offer_id.clone()?;
    Some(OfferPostPersistRecord {
        offer_id,
        market_id: ctx.market.market_id.clone(),
        side: side.to_string(),
        size_base_units,
        publish_venue: ctx.publish_venue.clone(),
        resolved_base_asset_id: ctx.resolved_base_asset_id.clone(),
        resolved_quote_asset_id: ctx.resolved_quote_asset_id.clone(),
        created_extra: json!({"execution_mode": execution_mode}),
    })
}

fn strategy_offer_execution_payload(record: &OfferPostPersistRecord) -> Value {
    let mut audit_event = json!({
        "market_id": record.market_id,
        "planned_count": 1,
        "executed_count": 1,
        "items": [{
            "size": record.size_base_units,
            "side": record.side,
            "status": "executed",
            "reason": format!("{}_post_success", record.publish_venue),
            "offer_id": record.offer_id,
            "attempts": 1,
        }],
        "venue": record.publish_venue,
        "resolved_base_asset_id": record.resolved_base_asset_id,
        "resolved_quote_asset_id": record.resolved_quote_asset_id,
    });
    if let Value::Object(extra) = &record.created_extra {
        if let Value::Object(audit_obj) = &mut audit_event {
            for (key, value) in extra {
                audit_obj.insert(key.clone(), value.clone());
            }
        }
    }
    audit_event
}

pub fn persist_post_records_if_enabled(
    home_dir: &Path,
    persist_results: bool,
    dry_run: bool,
    records: &[OfferPostPersistRecord],
) -> SignerResult<()> {
    if !persist_results || dry_run || records.is_empty() {
        return Ok(());
    }
    let db_path = state_db_path_for_home(home_dir);
    let store = SqliteStore::open(&db_path)?;
    for record in records {
        upsert_offer_post_record(&store, record)?;
        audit_market_cycle(
            &store,
            Level::INFO,
            STRATEGY_OFFER_EXECUTION,
            &strategy_offer_execution_payload(record),
            &record.market_id,
            "strategy offer executed",
        )?;
    }
    Ok(())
}

fn post_failure_payload(
    market_id: &str,
    publish_venue: &str,
    error: &str,
    offer_ref: Option<&str>,
) -> Value {
    let mut payload = json!({
        "market_id": market_id,
        "venue": publish_venue,
        "error": error,
        "planned_count": 1,
        "executed_count": 0,
    });
    if let Some(offer_ref) = offer_ref {
        if let Value::Object(obj) = &mut payload {
            obj.insert("offer_ref".to_string(), json!(offer_ref));
        }
    }
    payload
}

pub fn persist_post_failure_if_enabled(
    home_dir: &Path,
    persist_results: bool,
    dry_run: bool,
    market_id: &str,
    publish_venue: &str,
    error: &str,
    offer_ref: Option<&str>,
) -> SignerResult<()> {
    let payload = post_failure_payload(market_id, publish_venue, error, offer_ref);
    if !persist_results || dry_run {
        trace_audit_outcome(
            Level::WARN,
            LogContext::OFFER_POST,
            OFFER_POST_FAILURE,
            &payload,
            Some(market_id),
            "offer post failed",
        );
        return Ok(());
    }
    let db_path = state_db_path_for_home(home_dir);
    let store = SqliteStore::open(&db_path)?;
    audit_and_trace(
        &store,
        Level::WARN,
        LogContext::OFFER_POST,
        OFFER_POST_FAILURE,
        &payload,
        Some(market_id),
        "offer post failed",
    )
}

pub fn record_post_iteration_failure(
    request: &BuildAndPostOfferRequest,
    ctx: &ResolvedBuildAndPostContext,
    error: &str,
    offer_ref: Option<&str>,
) -> SignerResult<()> {
    persist_post_failure_if_enabled(
        &ctx.program.home_dir,
        request.run.persist_results,
        request.run.dry_run,
        &ctx.market.market_id,
        &ctx.publish_venue,
        error,
        offer_ref,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strategy_offer_execution_payload_includes_execution_mode() {
        let record = OfferPostPersistRecord {
            offer_id: "offer-1".to_string(),
            market_id: "m1".to_string(),
            side: "sell".to_string(),
            size_base_units: 5,
            publish_venue: "dexie".to_string(),
            resolved_base_asset_id: "a1".to_string(),
            resolved_quote_asset_id: "xch".to_string(),
            created_extra: json!({"execution_mode": "direct"}),
        };
        let payload = strategy_offer_execution_payload(&record);
        assert_eq!(
            payload.get("execution_mode").and_then(Value::as_str),
            Some("direct")
        );
    }
}
