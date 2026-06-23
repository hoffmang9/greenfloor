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

pub struct PostBatchEmitter<'a> {
    ctx: &'a ResolvedBuildAndPostContext,
}

impl<'a> PostBatchEmitter<'a> {
    #[must_use]
    pub fn new(ctx: &'a ResolvedBuildAndPostContext) -> Self {
        Self { ctx }
    }

    pub fn trace_iteration(&self, outcome: &str, offer_ref: &str) {
        crate::trace_event!(
            INFO,
            LogContext::OFFER_POST,
            OFFER_POST_ITERATION,
            {
                market_id = self.ctx.market.market_id.as_str(),
                outcome = outcome,
                publish_venue = self.ctx.publish_venue.as_str(),
                offer_ref = offer_ref,
            };
            "offer post iteration"
        );
    }

    pub fn trace_failure(&self, error: &str, offer_ref: Option<&str>) {
        let payload = self.failure_payload(error, offer_ref);
        LogContext::OFFER_POST.dual_trace(
            Level::WARN,
            "offer post failed",
            OFFER_POST_FAILURE,
            &payload,
            Some(self.ctx.market.market_id.as_str()),
        );
    }

    pub fn trace_completed(
        &self,
        outcome: &str,
        publish_attempts: usize,
        publish_failures: u32,
        dry_run: bool,
    ) {
        let level = match outcome {
            "success" => Level::INFO,
            "failure" => Level::ERROR,
            _ => Level::WARN,
        };
        crate::trace_event!(
            level = level,
            LogContext::OFFER_POST,
            OFFER_POST_COMPLETED,
            {
                market_id = self.ctx.market.market_id.as_str(),
                outcome = outcome,
                publish_attempts = publish_attempts,
                publish_failures = publish_failures,
                dry_run = dry_run,
            };
            "build-and-post-offer completed"
        );
    }

    pub fn flush(
        &self,
        store: &SqliteStore,
        records: &[OfferPostPersistRecord],
        failures: &[PostFailureAudit],
    ) -> SignerResult<()> {
        let mut deferred_traces = Vec::new();
        store.immediate_transaction("post flush", |store| {
            for record in records {
                upsert_offer_post_record(store, record)?;
            }
            for failure in failures {
                audit_row_defer_dual(&mut deferred_traces, store, self.deferred_failure(failure))?;
            }
            for record in records {
                audit_row_defer_dual(
                    &mut deferred_traces,
                    store,
                    Self::deferred_execution(record),
                )?;
            }
            Ok(())
        })?;
        emit_deferred_dual_traces(&deferred_traces);
        Ok(())
    }

    fn deferred_failure(&self, failure: &PostFailureAudit) -> DeferredDualEmit {
        DeferredDualEmit::new(
            LogContext::OFFER_POST,
            Level::WARN,
            "offer post failed",
            OFFER_POST_FAILURE,
            self.failure_payload(&failure.error, failure.offer_ref.as_deref()),
            Some(self.ctx.market.market_id.clone()),
        )
    }

    fn deferred_execution(record: &OfferPostPersistRecord) -> DeferredDualEmit {
        DeferredDualEmit::new(
            LogContext::MARKET_CYCLE,
            Level::INFO,
            "strategy offer executed",
            STRATEGY_OFFER_EXECUTION,
            strategy_offer_execution_payload(record),
            Some(record.market_id.clone()),
        )
    }

    fn failure_payload(&self, error: &str, offer_ref: Option<&str>) -> Value {
        let mut payload = json!({
            "market_id": self.ctx.market.market_id.as_str(),
            "venue": self.ctx.publish_venue.as_str(),
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
}

pub fn apply_post_iteration_outcome(
    target: PostEmitTarget,
    emitter: &PostBatchEmitter<'_>,
    outcome: PostIterationOutcome,
    batch: &mut PostIterationBatch,
) {
    match outcome {
        PostIterationOutcome::Preview(preview) => {
            let offer_ref = preview
                .get("offer_prefix")
                .and_then(Value::as_str)
                .unwrap_or("");
            emitter.trace_iteration("preview", offer_ref);
            batch.built_offers_preview.push(preview);
        }
        PostIterationOutcome::Failure(failure) => {
            if target == PostEmitTarget::TraceOnly {
                emitter.trace_failure(&failure.error, None);
            } else {
                batch.failure_audits.push(PostFailureAudit {
                    error: failure.error.clone(),
                    offer_ref: None,
                });
            }
            batch.publish_failures += 1;
            batch
                .post_results
                .push(failure.to_venue_result(&emitter.ctx.publish_venue));
        }
        PostIterationOutcome::Success(success) => {
            if success.success {
                let offer_ref = success
                    .persist_record
                    .as_ref()
                    .map(|record| offer_log_ref(&record.offer_id))
                    .unwrap_or_default();
                emitter.trace_iteration("success", &offer_ref);
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
                    emitter.trace_failure(&error, offer_ref.as_deref());
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
    if let Some(mode) = record.execution_mode {
        if let Value::Object(audit_obj) = &mut audit_event {
            audit_obj.insert("execution_mode".to_string(), json!(mode.to_string()));
        }
    }
    audit_event
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offer::types::{OfferExecutionMode, PresplitCancelFields};
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
            created_extra: json!({}),
            cancel_fields: PresplitCancelFields::default(),
            execution_mode: Some(OfferExecutionMode::Direct),
        };
        let payload = strategy_offer_execution_payload(&record);
        assert_eq!(
            payload.get("execution_mode").and_then(Value::as_str),
            Some("direct")
        );
    }
}
