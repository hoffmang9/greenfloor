use std::sync::OnceLock;

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use super::common::cached_class;

const CANCEL_POLICY_MODULE: &str = "greenfloor.core.cancel_policy";
const NOTIFICATIONS_MODULE: &str = "greenfloor.core.notifications";

static CANCEL_POLICY_DECISION_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static ALERT_EVENT_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static ALERT_STATE_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static LOW_INVENTORY_EVALUATION_CLS: OnceLock<Py<PyAny>> = OnceLock::new();

pub fn cancel_policy_decision_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &CANCEL_POLICY_DECISION_CLS,
        CANCEL_POLICY_MODULE,
        "CancelPolicyDecision",
    )
}

pub fn alert_event_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &ALERT_EVENT_CLS, NOTIFICATIONS_MODULE, "AlertEvent")
}

pub fn alert_state_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &ALERT_STATE_CLS, NOTIFICATIONS_MODULE, "AlertState")
}

pub fn low_inventory_evaluation_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &LOW_INVENTORY_EVALUATION_CLS,
        NOTIFICATIONS_MODULE,
        "LowInventoryEvaluation",
    )
}

pub fn cancel_policy_decision_to_py<'py>(
    py: Python<'py>,
    decision: &signer_core::CancelPolicyDecision,
) -> PyResult<Bound<'py, PyAny>> {
    let cls = cancel_policy_decision_class(py)?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("eligible", decision.eligible)?;
    kwargs.set_item("triggered", decision.triggered)?;
    kwargs.set_item("reason", &decision.reason)?;
    kwargs.set_item("move_bps", decision.move_bps)?;
    kwargs.set_item("threshold_bps", decision.threshold_bps)?;
    cls.call((), Some(&kwargs))
}

pub fn alert_event_to_py<'py>(
    py: Python<'py>,
    event: &signer_core::AlertEvent,
) -> PyResult<Bound<'py, PyAny>> {
    let cls = alert_event_class(py)?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("market_id", &event.market_id)?;
    kwargs.set_item("ticker", &event.ticker)?;
    kwargs.set_item("remaining_amount", event.remaining_amount)?;
    kwargs.set_item("receive_address", &event.receive_address)?;
    kwargs.set_item("reason", &event.reason)?;
    cls.call((), Some(&kwargs))
}

pub fn alert_state_to_py<'py>(
    py: Python<'py>,
    state: &signer_core::AlertState,
) -> PyResult<Bound<'py, PyAny>> {
    let cls = alert_state_class(py)?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("is_low", state.is_low)?;
    kwargs.set_item("last_alert_at_unix", state.last_alert_at_unix)?;
    cls.call((), Some(&kwargs))
}

pub fn low_inventory_evaluation_to_py<'py>(
    py: Python<'py>,
    evaluation: &signer_core::LowInventoryEvaluation,
) -> PyResult<Bound<'py, PyAny>> {
    let cls = low_inventory_evaluation_class(py)?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("state", alert_state_to_py(py, &evaluation.state)?)?;
    let event = match &evaluation.event {
        None => py.None().into_bound(py),
        Some(event) => alert_event_to_py(py, event)?.into_any(),
    };
    kwargs.set_item("event", event)?;
    cls.call((), Some(&kwargs))
}

pub fn low_inventory_input_from_py(
    input: &Bound<'_, PyAny>,
) -> PyResult<signer_core::LowInventoryInput> {
    Ok(signer_core::LowInventoryInput {
        now_unix: input.getattr("now_unix")?.extract()?,
        low_inventory_enabled: input.getattr("low_inventory_enabled")?.extract()?,
        program_default_threshold: input.getattr("program_default_threshold")?.extract()?,
        clear_hysteresis_percent: input.getattr("clear_hysteresis_percent")?.extract()?,
        dedup_cooldown_seconds: input.getattr("dedup_cooldown_seconds")?.extract()?,
        market_enabled: input.getattr("market_enabled")?.extract()?,
        market_id: input.getattr("market_id")?.extract()?,
        ticker: input.getattr("ticker")?.extract()?,
        receive_address: input.getattr("receive_address")?.extract()?,
        market_threshold: input.getattr("market_threshold")?.extract()?,
        low_watermark: input.getattr("low_watermark")?.extract()?,
        remaining: input.getattr("remaining")?.extract()?,
        state_is_low: input.getattr("state_is_low")?.extract()?,
        state_last_alert_at_unix: input.getattr("state_last_alert_at_unix")?.extract()?,
    })
}

pub fn open_offer_rows_from_py_list(offers: &Bound<'_, PyList>) -> PyResult<Vec<(String, i64)>> {
    let mut pairs = Vec::new();
    for item in offers.iter() {
        let offer_id = item
            .getattr("offer_id")
            .and_then(|value| value.extract::<String>())
            .unwrap_or_default();
        let status = item
            .getattr("status")
            .and_then(|value| value.extract::<i64>())
            .unwrap_or(-1);
        pairs.push((offer_id, status));
    }
    Ok(pairs)
}

pub fn string_list_to_py_list<'py>(
    py: Python<'py>,
    values: &[String],
) -> PyResult<Bound<'py, PyList>> {
    let list = PyList::empty(py);
    for value in values {
        list.append(value)?;
    }
    Ok(list)
}
