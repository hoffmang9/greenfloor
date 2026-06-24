use serde::ser::{SerializeStruct, Serializer};
use serde::Serialize;
use serde_json::{json, Value};

use crate::offer::bootstrap::{bootstrap_offer_gate_for_status, BootstrapPhaseStatus};
use crate::offer::bootstrap::{BootstrapFundingSource, BootstrapPhaseSnapshot, BootstrapPlan};

#[derive(Debug, Clone)]
pub struct BootstrapPhaseResult {
    phase_status: BootstrapPhaseStatus,
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
        state.serialize_field("status", self.status())?;
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
    funding: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_coin_id: Option<&'a str>,
    source_amount: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    combine_input_coin_ids: Option<&'a [String]>,
    output_amounts_base_units: &'a [i64],
    total_output_amount: i64,
    change_amount: i64,
    output_count: usize,
}

impl<'a> From<&'a BootstrapPlan> for BootstrapPlanOutput<'a> {
    fn from(plan: &'a BootstrapPlan) -> Self {
        let (funding, source_coin_id, combine_input_coin_ids) = match &plan.funding {
            BootstrapFundingSource::SingleCoin { coin_id, .. } => {
                ("single_coin", Some(coin_id.as_str()), None)
            }
            BootstrapFundingSource::CombineFirst(prereq) => (
                "combine_first",
                None,
                Some(prereq.input_coin_ids.as_slice()),
            ),
        };
        Self {
            funding,
            source_coin_id,
            source_amount: plan.source_amount(),
            combine_input_coin_ids,
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
    pub fn status(&self) -> &'static str {
        self.phase_status.as_str()
    }

    #[must_use]
    pub fn to_operator_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({}))
    }

    pub(super) fn from_snapshot(snapshot: BootstrapPhaseSnapshot) -> Self {
        Self {
            phase_status: BootstrapPhaseStatus::from_snapshot_status(snapshot.status),
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
            phase_status: BootstrapPhaseStatus::Skipped,
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
        bootstrap_offer_gate_for_status(self.phase_status, &self.reason, self.ready).block_error()
    }

    pub(super) fn failed(failure: BootstrapPhaseFailure) -> Self {
        Self {
            phase_status: BootstrapPhaseStatus::Failed,
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
}

#[cfg(test)]
mod tests {
    use super::{BootstrapPhaseResult, BootstrapPlanOutput};
    use crate::offer::bootstrap::{
        BaseUnits, BootstrapCombineInputs, BootstrapFundingSource, BootstrapPhaseSnapshot,
        BootstrapPlan,
    };

    #[test]
    fn from_snapshot_block_error_matches_snapshot_gate() {
        for (status, reason, ready) in [
            ("failed", "bootstrap_invalid_ladder", false),
            ("skipped", "already_ready", false),
            ("skipped", "seed_missing", false),
            ("executed", "bootstrap_submitted", true),
            ("executed", "split_submitted", false),
        ] {
            let snapshot = BootstrapPhaseSnapshot {
                status,
                reason: reason.to_string(),
                ready,
            };
            assert_eq!(
                BootstrapPhaseResult::from_snapshot(snapshot.clone()).offer_creation_block_error(),
                snapshot.offer_creation_block_error(),
                "status={status} reason={reason} ready={ready}"
            );
        }
    }

    #[test]
    fn plan_output_omits_source_coin_id_for_combine_first() {
        let plan = BootstrapPlan {
            funding: BootstrapFundingSource::CombineFirst(BootstrapCombineInputs {
                input_coin_ids: vec!["coin-a".to_string(), "coin-b".to_string()],
                target_amount: BaseUnits::new(100),
                selected_total: BaseUnits::new(100),
                exact_match: true,
                cap_applied: false,
            }),
            output_amounts_base_units: vec![100],
            total_output_amount: 100,
            change_amount: 0,
            deficits: Vec::new(),
        };
        let output = BootstrapPlanOutput::from(&plan);
        assert_eq!(output.funding, "combine_first");
        assert!(output.source_coin_id.is_none());
        assert_eq!(
            output.combine_input_coin_ids,
            Some(["coin-a".to_string(), "coin-b".to_string()].as_slice())
        );
    }
}
