use serde_json::{json, Value};

use crate::offer::bootstrap::{BootstrapPhaseSnapshot, BootstrapPlan};
use crate::offer::publish::{bootstrap_offer_gate, BootstrapOfferGate};

#[derive(Debug, Clone)]
pub struct BootstrapPhaseResult {
    pub status: String,
    pub reason: String,
    pub ready: bool,
    pub fee_mojos: u64,
    pub fee_source: String,
    pub fee_lookup_error: Option<String>,
    pub wait_error: Option<String>,
    pub split_result: Value,
    pub wait_events: Vec<Value>,
    pub plan: Option<BootstrapPlan>,
}

impl BootstrapPhaseResult {
    pub fn to_operator_json(&self) -> Value {
        let mut payload = json!({
            "status": self.status,
            "reason": self.reason,
            "ready": self.ready,
            "fee_mojos": self.fee_mojos,
            "fee_source": self.fee_source,
            "fee_lookup_error": self.fee_lookup_error,
        });
        if let Some(wait_error) = &self.wait_error {
            payload["wait_error"] = json!(wait_error);
        }
        if !self.split_result.is_null() && self.split_result != json!({}) {
            payload["split_result"] = self.split_result.clone();
        }
        if !self.wait_events.is_empty() {
            payload["wait_events"] = Value::Array(self.wait_events.clone());
        }
        if let Some(plan) = &self.plan {
            payload["plan"] = json!({
                "source_coin_id": plan.source_coin_id,
                "source_amount": plan.source_amount,
                "output_amounts_base_units": plan.output_amounts_base_units,
                "total_output_amount": plan.total_output_amount,
                "change_amount": plan.change_amount,
                "output_count": plan.output_amounts_base_units.len(),
            });
        }
        payload
    }

    pub(super) fn from_snapshot(snapshot: BootstrapPhaseSnapshot) -> Self {
        Self {
            status: snapshot.status.to_string(),
            reason: snapshot.reason,
            ready: snapshot.ready,
            fee_mojos: 0,
            fee_source: String::new(),
            fee_lookup_error: None,
            wait_error: None,
            split_result: json!({}),
            wait_events: Vec::new(),
            plan: None,
        }
    }

    pub(crate) fn skipped(reason: impl Into<String>) -> Self {
        Self {
            status: "skipped".to_string(),
            reason: reason.into(),
            ready: false,
            fee_mojos: 0,
            fee_source: String::new(),
            fee_lookup_error: None,
            wait_error: None,
            split_result: json!({}),
            wait_events: Vec::new(),
            plan: None,
        }
    }

    pub fn offer_creation_gate(&self) -> BootstrapOfferGate {
        bootstrap_offer_gate(&self.status, &self.reason, self.ready)
    }

    pub(super) fn failed(failure: BootstrapPhaseFailure) -> Self {
        Self {
            status: "failed".to_string(),
            reason: failure.reason,
            ready: false,
            fee_mojos: failure.fee_mojos,
            fee_source: failure.fee_source,
            fee_lookup_error: failure.fee_lookup_error,
            wait_error: failure.wait_error,
            split_result: failure.split_result,
            wait_events: failure.wait_events,
            plan: failure.plan,
        }
    }
}

pub(super) struct BootstrapPhaseFailure {
    pub reason: String,
    pub fee_mojos: u64,
    pub fee_source: String,
    pub fee_lookup_error: Option<String>,
    pub wait_error: Option<String>,
    pub split_result: Value,
    pub wait_events: Vec<Value>,
    pub plan: Option<BootstrapPlan>,
}

impl BootstrapPhaseFailure {
    pub(super) fn new(
        reason: impl Into<String>,
        fee_mojos: u64,
        fee_source: String,
        fee_lookup_error: Option<String>,
    ) -> Self {
        Self {
            reason: reason.into(),
            fee_mojos,
            fee_source,
            fee_lookup_error,
            wait_error: None,
            split_result: json!({}),
            wait_events: Vec::new(),
            plan: None,
        }
    }

    pub(super) fn with_plan(mut self, plan: BootstrapPlan) -> Self {
        self.plan = Some(plan);
        self
    }

    pub(super) fn with_wait_error(mut self, wait_error: impl Into<String>) -> Self {
        self.wait_error = Some(wait_error.into());
        self
    }

    pub(super) fn with_split_result(mut self, split_result: Value) -> Self {
        self.split_result = split_result;
        self
    }
}

pub fn bootstrap_blocks_offer(result: &BootstrapPhaseResult) -> Option<String> {
    result.offer_creation_gate().block_error(&result.reason)
}
