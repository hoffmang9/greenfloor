//! Trace mirror contract for dual-emit: structured scalar fields vs redacted payload blob.

use serde_json::{Map, Value};
use tracing::Level;

use super::emit::LogContext;
use super::redact::{offer_log_ref, redact_json_for_log, truncate_id};

const BLOB_MIRROR_BYTE_LIMIT: usize = 512;

/// Returns true when the audit payload should mirror to trace as one redacted JSON blob.
#[must_use]
pub fn payload_use_blob_mirror(payload: &Value) -> bool {
    let redacted = redact_json_for_log(payload);
    if redacted.to_string().len() > BLOB_MIRROR_BYTE_LIMIT {
        return true;
    }
    match redacted.as_object() {
        Some(obj) => obj.values().any(Value::is_array) || obj.values().any(Value::is_object),
        None => true,
    }
}

#[derive(Debug, Default)]
struct StructuredTraceFields {
    error: String,
    outcome: String,
    reason: String,
    source: String,
    signal: String,
    venue: String,
    offer_ref: String,
    action_count: i64,
    plan_count: i64,
    executable_count: i64,
    coin_count: i64,
    count: i64,
    price_usd: f64,
}

impl StructuredTraceFields {
    fn from_payload(payload: &Value) -> Self {
        let redacted = redact_json_for_log(payload);
        let Some(obj) = redacted.as_object() else {
            return Self::default();
        };
        let mut fields = Self {
            error: string_field(obj, "error"),
            outcome: string_field(obj, "outcome"),
            reason: string_field(obj, "reason"),
            source: string_field(obj, "source"),
            signal: string_field(obj, "signal"),
            venue: string_field(obj, "venue"),
            offer_ref: offer_ref_field(obj),
            action_count: i64_field(obj, &["action_count", "planned_count"]),
            plan_count: i64_field(obj, &["plan_count"]),
            executable_count: i64_field(obj, &["executable_count", "executed_count"]),
            coin_count: i64_field(obj, &["coin_count"]),
            count: i64_field(obj, &["count", "new_tx_ids", "invalid_bucket_count"]),
            price_usd: f64_field(obj, "price_usd"),
        };
        if fields.offer_ref.is_empty() {
            if let Some(offer_id) = obj.get("offer_id").and_then(Value::as_str) {
                fields.offer_ref = truncate_id(offer_id, 8);
            }
        }
        fields
    }
}

fn string_field(obj: &Map<String, Value>, key: &str) -> String {
    obj.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn i64_field(obj: &Map<String, Value>, keys: &[&str]) -> i64 {
    for key in keys {
        if let Some(value) = obj.get(*key).and_then(value_as_i64) {
            return value;
        }
    }
    -1
}

fn f64_field(obj: &Map<String, Value>, key: &str) -> f64 {
    obj.get(key).and_then(Value::as_f64).unwrap_or(-1.0)
}

fn value_as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|v| i64::try_from(v).ok()))
}

fn offer_ref_field(obj: &Map<String, Value>) -> String {
    if let Some(text) = obj.get("offer_text").and_then(Value::as_str) {
        return offer_log_ref(text);
    }
    if let Some(prefix) = obj.get("offer_ref").and_then(Value::as_str) {
        return prefix.to_string();
    }
    String::new()
}

fn trace_blob_mirror(
    level: Level,
    ctx: LogContext,
    audit_event_type: &str,
    payload: &Value,
    market_id: Option<&str>,
    trace_message: &'static str,
) {
    let payload_text = redact_json_for_log(payload).to_string();
    crate::event_at_level!(
        level,
        service = ctx.service,
        event = audit_event_type,
        phase = ctx.phase,
        market_id = market_id.unwrap_or(""),
        payload = %payload_text,
        trace_message
    );
}

fn trace_structured_mirror(
    level: Level,
    ctx: LogContext,
    audit_event_type: &str,
    payload: &Value,
    market_id: Option<&str>,
    trace_message: &'static str,
) {
    let fields = StructuredTraceFields::from_payload(payload);
    let market = market_id
        .map(str::to_string)
        .or_else(|| {
            payload
                .get("market_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();
    crate::event_at_level!(
        level,
        service = ctx.service,
        event = audit_event_type,
        phase = ctx.phase,
        market_id = market.as_str(),
        error = fields.error.as_str(),
        outcome = fields.outcome.as_str(),
        reason = fields.reason.as_str(),
        source = fields.source.as_str(),
        signal = fields.signal.as_str(),
        venue = fields.venue.as_str(),
        offer_ref = fields.offer_ref.as_str(),
        action_count = fields.action_count,
        plan_count = fields.plan_count,
        executable_count = fields.executable_count,
        coin_count = fields.coin_count,
        count = fields.count,
        price_usd = fields.price_usd,
        trace_message
    );
}

/// Mirror one audit outcome to trace using the tier-2 contract for this payload shape.
pub fn trace_audit_mirror(
    level: Level,
    ctx: LogContext,
    audit_event_type: &str,
    payload: &Value,
    market_id: Option<&str>,
    trace_message: &'static str,
) {
    if payload_use_blob_mirror(payload) {
        trace_blob_mirror(
            level,
            ctx,
            audit_event_type,
            payload,
            market_id,
            trace_message,
        );
    } else {
        trace_structured_mirror(
            level,
            ctx,
            audit_event_type,
            payload,
            market_id,
            trace_message,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operator_log::emit::trace_capture::TraceCapture;

    #[test]
    fn scalar_payload_uses_structured_mirror() {
        let capture = TraceCapture::install();
        let payload = serde_json::json!({
            "market_id": "m1",
            "error": "dexie_http_error:timeout",
        });
        assert!(!payload_use_blob_mirror(&payload));
        trace_audit_mirror(
            Level::WARN,
            LogContext::MARKET_CYCLE,
            "dexie_offers_error",
            &payload,
            Some("m1"),
            "dexie offers fetch failed",
        );
        let logs = capture.logs();
        assert!(logs.contains("dexie_http_error:timeout"));
        assert!(!logs.contains("payload="));
    }

    #[test]
    fn nested_payload_uses_blob_mirror() {
        let capture = TraceCapture::install();
        let payload = serde_json::json!({
            "market_id": "m1",
            "plans": [{"op_type": "split", "op_count": 1}],
        });
        assert!(payload_use_blob_mirror(&payload));
        trace_audit_mirror(
            Level::INFO,
            LogContext::MARKET_CYCLE,
            "coin_ops_plan",
            &payload,
            Some("m1"),
            "coin ops plan",
        );
        let logs = capture.logs();
        assert!(logs.contains("payload="));
        assert!(logs.contains("coin_ops_plan"));
    }
}
