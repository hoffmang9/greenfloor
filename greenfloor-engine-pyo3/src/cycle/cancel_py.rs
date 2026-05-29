use pyo3::prelude::*;
use pyo3::types::PyList;
use engine_core::{
    abs_move_bps, cancel_move_threshold_bps, collect_open_offer_ids_for_cancel,
    evaluate_cancel_policy_decision,
};

use crate::py_utils::{
    cancel_policy_decision_to_py, open_offer_rows_from_py_list, string_list_to_py_list,
};

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
    Ok(cancel_policy_decision_to_py(py, &decision)?.into())
}

#[pyfunction]
#[pyo3(name = "collect_open_offer_ids_for_cancel")]
fn collect_open_offer_ids_for_cancel_py(
    py: Python<'_>,
    offers: &Bound<'_, PyList>,
) -> PyResult<Py<PyAny>> {
    let pairs = open_offer_rows_from_py_list(offers)?;
    let offer_ids = collect_open_offer_ids_for_cancel(&pairs);
    Ok(string_list_to_py_list(py, &offer_ids)?.into())
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(abs_move_bps_py, m)?)?;
    m.add_function(wrap_pyfunction!(cancel_move_threshold_bps_py, m)?)?;
    m.add_function(wrap_pyfunction!(evaluate_cancel_policy_decision_py, m)?)?;
    m.add_function(wrap_pyfunction!(collect_open_offer_ids_for_cancel_py, m)?)?;
    Ok(())
}
