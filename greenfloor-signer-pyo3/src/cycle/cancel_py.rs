use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use signer_core::{
    abs_move_bps, cancel_move_threshold_bps, collect_open_offer_ids_for_cancel,
    evaluate_cancel_policy_decision,
};

fn cancel_policy_decision_to_py(
    py: Python<'_>,
    decision: &signer_core::CancelPolicyDecision,
) -> PyResult<Py<PyAny>> {
    let dict = PyDict::new(py);
    dict.set_item("eligible", decision.eligible)?;
    dict.set_item("triggered", decision.triggered)?;
    dict.set_item("reason", &decision.reason)?;
    dict.set_item("move_bps", decision.move_bps)?;
    dict.set_item("threshold_bps", decision.threshold_bps)?;
    Ok(dict.into())
}

#[pyfunction]
#[pyo3(name = "abs_move_bps")]
fn abs_move_bps_py(current: Option<f64>, previous: Option<f64>) -> Option<f64> {
    abs_move_bps(current, previous)
}

#[pyfunction]
#[pyo3(name = "cancel_move_threshold_bps")]
fn cancel_move_threshold_bps_py(market_threshold: Option<i64>, env_threshold: Option<i64>) -> i64 {
    cancel_move_threshold_bps(market_threshold, env_threshold)
}

#[pyfunction]
#[pyo3(name = "evaluate_cancel_policy_decision")]
fn evaluate_cancel_policy_decision_py(
    py: Python<'_>,
    quote_asset_type: &str,
    cancel_policy_stable_vs_unstable: bool,
    current_xch_price_usd: Option<f64>,
    previous_xch_price_usd: Option<f64>,
    market_threshold: Option<i64>,
    env_threshold: Option<i64>,
) -> PyResult<Py<PyAny>> {
    let decision = evaluate_cancel_policy_decision(
        quote_asset_type,
        cancel_policy_stable_vs_unstable,
        current_xch_price_usd,
        previous_xch_price_usd,
        market_threshold,
        env_threshold,
    );
    cancel_policy_decision_to_py(py, &decision)
}

#[pyfunction]
#[pyo3(name = "collect_open_offer_ids_for_cancel")]
fn collect_open_offer_ids_for_cancel_py(
    py: Python<'_>,
    offers: &Bound<'_, PyList>,
) -> PyResult<Py<PyAny>> {
    let list = PyList::empty(py);
    for (index, item) in offers.iter().enumerate() {
        let offer = item
            .cast::<PyDict>()
            .map_err(|_| PyValueError::new_err(format!("offer item {index} must be a dict")))?;
        let offer_id = offer
            .get_item("id")?
            .map(|value| value.extract::<String>())
            .transpose()?
            .unwrap_or_default();
        let status = offer
            .get_item("status")?
            .map(|value| value.extract::<i64>())
            .transpose()?
            .unwrap_or(-1);
        if let Some(normalized_id) = collect_open_offer_ids_for_cancel(&offer_id, status) {
            list.append(normalized_id)?;
        }
    }
    Ok(list.into())
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(abs_move_bps_py, m)?)?;
    m.add_function(wrap_pyfunction!(cancel_move_threshold_bps_py, m)?)?;
    m.add_function(wrap_pyfunction!(evaluate_cancel_policy_decision_py, m)?)?;
    m.add_function(wrap_pyfunction!(collect_open_offer_ids_for_cancel_py, m)?)?;
    Ok(())
}
