use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::py_utils::{dict_from_json_value, to_py_err};

use signer_core::{
    apply_offer_signal, expiry_seconds_for_action, OfferLifecycleState, OfferSignal,
};

pub(crate) fn parse_lifecycle_state(value: &str) -> PyResult<OfferLifecycleState> {
    match value.trim() {
        "open" => Ok(OfferLifecycleState::Open),
        "mempool_observed" => Ok(OfferLifecycleState::MempoolObserved),
        "tx_block_confirmed" => Ok(OfferLifecycleState::TxBlockConfirmed),
        "refresh_due" => Ok(OfferLifecycleState::RefreshDue),
        "expired" => Ok(OfferLifecycleState::Expired),
        other => Err(PyValueError::new_err(format!(
            "unknown offer lifecycle state: {other}"
        ))),
    }
}

pub(crate) fn parse_offer_signal(value: &str) -> PyResult<OfferSignal> {
    match value.trim() {
        "mempool_seen" => Ok(OfferSignal::MempoolSeen),
        "tx_confirmed" => Ok(OfferSignal::TxConfirmed),
        "expiry_near" => Ok(OfferSignal::ExpiryNear),
        "expired" => Ok(OfferSignal::Expired),
        "refresh_posted" => Ok(OfferSignal::RefreshPosted),
        other => Err(PyValueError::new_err(format!(
            "unknown offer signal: {other}"
        ))),
    }
}

#[pyfunction]
#[pyo3(name = "apply_offer_signal")]
fn apply_offer_signal_py(state: &str, signal: &str) -> PyResult<Py<PyAny>> {
    let state = parse_lifecycle_state(state)?;
    let signal = parse_offer_signal(signal)?;
    let transition = apply_offer_signal(state, signal);
    Python::attach(|py| {
        dict_from_json_value(py, serde_json::to_value(&transition).map_err(to_py_err)?)
    })
}

#[pyfunction]
#[pyo3(name = "expiry_seconds_for_action")]
fn expiry_seconds_for_action_py(expiry_unit: &str, expiry_value: i64) -> PyResult<Option<i64>> {
    Ok(expiry_seconds_for_action(expiry_unit, expiry_value))
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(apply_offer_signal_py, m)?)?;
    m.add_function(wrap_pyfunction!(expiry_seconds_for_action_py, m)?)?;
    Ok(())
}
