use serde_json::{json, Value};
use tracing::Level;

use crate::error::SignerResult;
use crate::operator_log::{
    audit_row_defer_dual, emit_deferred_dual_traces, offer_log_ref, DeferredDualEmit, LogContext,
    OFFER_POST_COMPLETED, OFFER_POST_FAILURE, OFFER_POST_ITERATION, STRATEGY_OFFER_EXECUTION,
};
use crate::storage::{upsert_offer_post_record, OfferPostPersistRecord, SqliteStore};

use super::context::ResolvedBuildAndPostContext;
use super::types::PostIterationOutcome;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostEmitTarget {
    TraceOnly,
    TraceAndStore,
}

impl PostEmitTarget {
    #[must_use]
    pub fn from_run(persist_results: bool, dry_run: bool) -> Self {
        if persist_results && !dry_run {
            Self::TraceAndStore
        } else {
            Self::TraceOnly
        }
    }
}

pub struct PostFailureAudit {
    pub error: String,
    pub offer_ref: Option<String>,
}

pub struct PostIterationBatch {
    pub post_results: Vec<Value>,
    pub built_offers_preview: Vec<Value>,
    pub bootstrap_actions: Vec<Value>,
    pub publish_failures: u32,
    pub persist_records: Vec<OfferPostPersistRecord>,
    pub failure_audits: Vec<PostFailureAudit>,
}

pub fn flush_post_batch(
    store: &SqliteStore,
    ctx: &ResolvedBuildAndPostContext,
    records: &[OfferPostPersistRecord],
    failures: &[PostFailureAudit],
) -> SignerResult<()> {
    let mut deferred_traces = Vec::new();
    store.immediate_transaction("post flush", |store| {
        for record in records {
            upsert_offer_post_record(store, record)?;
        }
        for failure in failures {
            let payload = post_failure_payload(
                &ctx.market.market_id,
                &ctx.publish_venue,
                &failure.error,
                failure.offer_ref.as_deref(),
            );
            audit_row_defer_dual(
                &mut deferred_traces,
                store,
                DeferredDualEmit {
                    ctx: LogContext::OFFER_POST,
                    level: Level::WARN,
                    trace_message: "offer post failed",
                    audit_event_type: OFFER_POST_FAILURE,
                    payload,
                    market_id: Some(ctx.market.market_id.clone()),
                },
            )?;
        }
        for record in records {
            let payload = strategy_offer_execution_payload(record);
            audit_row_defer_dual(
                &mut deferred_traces,
                store,
                DeferredDualEmit {
                    ctx: LogContext::MARKET_CYCLE,
                    level: Level::INFO,
                    trace_message: "strategy offer executed",
                    audit_event_type: STRATEGY_OFFER_EXECUTION,
                    payload,
                    market_id: Some(record.market_id.clone()),
                },
            )?;
        }
        Ok(())
    })?;
    emit_deferred_dual_traces(&deferred_traces);
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

fn trace_post_failure(ctx: &ResolvedBuildAndPostContext, error: &str, offer_ref: Option<&str>) {
    let payload = post_failure_payload(&ctx.market.market_id, &ctx.publish_venue, error, offer_ref);
    let _ = LogContext::OFFER_POST.dual_trace(
        Level::WARN,
        "offer post failed",
        OFFER_POST_FAILURE,
        &payload,
        Some(&ctx.market.market_id),
    );
}

fn trace_post_iteration(outcome: &str, ctx: &ResolvedBuildAndPostContext, offer_ref: &str) {
    crate::trace_event!(
        INFO,
        LogContext::OFFER_POST,
        OFFER_POST_ITERATION,
        {
            market_id = ctx.market.market_id.as_str(),
            outcome = outcome,
            publish_venue = ctx.publish_venue.as_str(),
            offer_ref = offer_ref,
        };
        "offer post iteration"
    );
}

pub fn apply_post_iteration_outcome(
    target: PostEmitTarget,
    ctx: &ResolvedBuildAndPostContext,
    outcome: PostIterationOutcome,
    batch: &mut PostIterationBatch,
) {
    match outcome {
        PostIterationOutcome::Preview(preview) => {
            let offer_ref = preview
                .get("offer_prefix")
                .and_then(Value::as_str)
                .unwrap_or("");
            trace_post_iteration("preview", ctx, offer_ref);
            batch.built_offers_preview.push(preview);
        }
        PostIterationOutcome::Failure(failure) => {
            if target == PostEmitTarget::TraceOnly {
                trace_post_failure(ctx, &failure.error, None);
            } else {
                batch.failure_audits.push(PostFailureAudit {
                    error: failure.error.clone(),
                    offer_ref: None,
                });
            }
            batch.publish_failures += 1;
            batch
                .post_results
                .push(failure.to_venue_result(&ctx.publish_venue));
        }
        PostIterationOutcome::Success(success) => {
            if success.success {
                let offer_ref = success
                    .persist_record
                    .as_ref()
                    .map(|record| offer_log_ref(&record.offer_id))
                    .unwrap_or_default();
                trace_post_iteration("success", ctx, &offer_ref);
            } else {
                let error = success
                    .result
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("publish_failed")
                    .to_string();
                let offer_ref = success
                    .persist_record
                    .as_ref()
                    .map(|record| offer_log_ref(&record.offer_id));
                if target == PostEmitTarget::TraceOnly {
                    trace_post_failure(ctx, &error, offer_ref.as_deref());
                } else {
                    batch
                        .failure_audits
                        .push(PostFailureAudit { error, offer_ref });
                }
                batch.publish_failures += 1;
            }
            batch.post_results.push(success.to_venue_result());
            if let Some(record) = success.persist_record {
                batch.persist_records.push(record);
            }
        }
    }
}

pub fn trace_offer_post_completed(
    outcome: &str,
    market_id: &str,
    publish_attempts: usize,
    publish_failures: u32,
    dry_run: bool,
) {
    let level = match outcome {
        "success" => Level::INFO,
        "failure" => Level::ERROR,
        _ => Level::WARN,
    };
    crate::trace_event_at_level!(
        level,
        LogContext::OFFER_POST,
        OFFER_POST_COMPLETED,
        {
            market_id = market_id,
            outcome = outcome,
            publish_attempts = publish_attempts,
            publish_failures = publish_failures,
            dry_run = dry_run,
        };
        "build-and-post-offer completed"
    );
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
