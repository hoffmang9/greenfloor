use serde::ser::{SerializeStruct, Serializer};
use serde::Serialize;
use serde_json::{json, Value};

use crate::offer::bootstrap::bootstrap_offer_gate;
use crate::offer::bootstrap::{BootstrapPhaseSnapshot, BootstrapPlan};

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

impl Serialize for BootstrapPhaseResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let field_count = 6
            + usize::from(self.fee_lookup_error.is_some())
            + usize::from(self.wait_error.is_some())
            + usize::from(!is_empty_json_value(&self.split_result))
            + usize::from(!self.wait_events.is_empty())
            + usize::from(self.plan.is_some());
        let mut state = serializer.serialize_struct("BootstrapPhaseResult", field_count)?;
        state.serialize_field("status", &self.status)?;
        state.serialize_field("reason", &self.reason)?;
        state.serialize_field("ready", &self.ready)?;
        state.serialize_field("fee_mojos", &self.fee_mojos)?;
        state.serialize_field("fee_source", &self.fee_source)?;
        if let Some(value) = &self.fee_lookup_error {
            state.serialize_field("fee_lookup_error", value)?;
        }
        if let Some(value) = &self.wait_error {
            state.serialize_field("wait_error", value)?;
        }
        if !is_empty_json_value(&self.split_result) {
            state.serialize_field("split_result", &self.split_result)?;
        }
        if !self.wait_events.is_empty() {
            state.serialize_field("wait_events", &self.wait_events)?;
        }
        if let Some(plan) = &self.plan {
            state.serialize_field("plan", &BootstrapPlanOutput::from(plan))?;
        }
        state.end()
    }
}

#[derive(Serialize)]
struct BootstrapPlanOutput<'a> {
    source_coin_id: &'a str,
    source_amount: i64,
    output_amounts_base_units: &'a [i64],
    total_output_amount: i64,
    change_amount: i64,
    output_count: usize,
}

impl<'a> From<&'a BootstrapPlan> for BootstrapPlanOutput<'a> {
    fn from(plan: &'a BootstrapPlan) -> Self {
        Self {
            source_coin_id: &plan.source_coin_id,
            source_amount: plan.source_amount,
            output_amounts_base_units: &plan.output_amounts_base_units,
            total_output_amount: plan.total_output_amount,
            change_amount: plan.change_amount,
            output_count: plan.output_amounts_base_units.len(),
        }
    }
}

fn is_empty_json_value(value: &Value) -> bool {
    value.is_null() || value == &json!({})
}

impl BootstrapPhaseResult {
    #[must_use]
    pub fn to_operator_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({}))
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

    /// Return manager bootstrap block reason text, or ``None`` when offer creation should continue.
    #[must_use]
    pub fn offer_creation_block_error(&self) -> Option<String> {
        bootstrap_offer_gate(&self.status, &self.reason, self.ready).block_error()
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
