use std::time::Instant;

use serde_json::{json, Value};

pub(crate) fn build_and_post_exit_code(publish_failures: u32) -> i32 {
    if publish_failures == 0 {
        0
    } else {
        2
    }
}

pub(super) enum PostIterationOutcome {
    Preview(Value),
    Failure(PostFailure),
    Success(PostAttemptSuccess),
}

#[derive(Debug, Clone)]
pub(super) struct PublishResult {
    pub success: bool,
    pub offer_id: Option<String>,
    pub body: Value,
}

#[derive(Debug)]
pub(super) struct PostFailure {
    pub error: String,
    pub started: Instant,
    pub create_phase_ms: Option<u64>,
    pub execution_mode: Option<String>,
    pub bootstrap: Option<Value>,
}

#[derive(Debug)]
pub(super) struct PostAttemptSuccess {
    pub publish_venue: String,
    pub result: Value,
    pub success: bool,
    pub persist_record: Option<crate::storage::OfferPostPersistRecord>,
}

impl PostAttemptSuccess {
    pub fn to_venue_result(&self) -> Value {
        json!({
            "venue": self.publish_venue,
            "result": self.result,
        })
    }
}

impl PostFailure {
    pub fn to_venue_result(&self, publish_venue: &str) -> Value {
        let mut result = json!({
            "success": false,
            "error": self.error,
            "timing_ms": timing_payload(
                self.started,
                self.create_phase_ms,
                self.create_phase_ms,
                None,
            ),
        });
        if let Some(execution_mode) = &self.execution_mode {
            result["execution_mode"] = json!(execution_mode);
        }
        if let Some(bootstrap) = &self.bootstrap {
            result["bootstrap"] = bootstrap.clone();
        }
        json!({
            "venue": publish_venue,
            "result": result,
        })
    }
}

impl PublishResult {
    pub fn from_adapter_body(body: Value) -> Self {
        let success = body.get("success").and_then(Value::as_bool) == Some(true);
        let offer_id = body
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        Self {
            success,
            offer_id,
            body,
        }
    }
}

pub(super) fn timing_payload(
    started: Instant,
    create_phase_ms: Option<u64>,
    create_total_ms: Option<u64>,
    publish_ms: Option<u64>,
) -> Value {
    json!({
        "create_phase_ms": create_phase_ms,
        "publish_ms": publish_ms,
        "total_ms": started.elapsed().as_millis().try_into().unwrap_or(0u64),
        "create_total_ms": create_total_ms.or(create_phase_ms),
    })
}
