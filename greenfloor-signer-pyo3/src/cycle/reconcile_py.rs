use pyo3::prelude::*;
use pyo3::types::PyDict;

use signer_core::{
    reconciled_state_from_dexie_status, resolve_missing_watched_offer_transition,
    resolve_watched_offer_transition, taker_fields, CycleOfferTransition,
};

use crate::py_utils::cycle_offer_transition_class;

pub fn cycle_offer_transition_to_py<'py>(
    py: Python<'py>,
    transition: &CycleOfferTransition,
) -> PyResult<Bound<'py, PyAny>> {
    let cls = cycle_offer_transition_class(py)?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("old_state", &transition.old_state)?;
    kwargs.set_item("new_state", &transition.new_state)?;
    kwargs.set_item("reason", &transition.reason)?;
    kwargs.set_item("signal_source", &transition.signal_source)?;
    match &transition.signal {
        Some(signal) => kwargs.set_item("signal", signal)?,
        None => kwargs.set_item("signal", py.None())?,
    }
    kwargs.set_item("changed", transition.changed)?;
    kwargs.set_item("immediate_requeue", transition.immediate_requeue)?;
    kwargs.set_item("coinset_tx_ids", &transition.coinset_tx_ids)?;
    kwargs.set_item("coinset_confirmed_tx_ids", &transition.coinset_confirmed_tx_ids)?;
    kwargs.set_item("coinset_mempool_tx_ids", &transition.coinset_mempool_tx_ids)?;
    cls.call((), Some(&kwargs))
}

#[pyfunction]
#[pyo3(name = "reconciled_state_from_dexie_status")]
fn reconciled_state_from_dexie_status_py(status: i64, current_state: &str) -> String {
    reconciled_state_from_dexie_status(status, current_state)
}

#[pyfunction]
#[pyo3(name = "resolve_missing_watched_offer_transition")]
fn resolve_missing_watched_offer_transition_py(
    py: Python<'_>,
    current_state: &str,
) -> PyResult<Py<PyAny>> {
    let transition = resolve_missing_watched_offer_transition(current_state);
    Ok(cycle_offer_transition_to_py(py, &transition)?.into())
}

#[pyfunction]
#[pyo3(name = "resolve_watched_offer_transition")]
fn resolve_watched_offer_transition_py(
    py: Python<'_>,
    current_state: &str,
    status: Option<i64>,
    coinset_tx_ids: Vec<String>,
    coinset_confirmed_tx_ids: Vec<String>,
    coinset_mempool_tx_ids: Vec<String>,
) -> PyResult<Py<PyAny>> {
    let transition = resolve_watched_offer_transition(
        current_state,
        status,
        coinset_tx_ids,
        coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids,
    );
    Ok(cycle_offer_transition_to_py(py, &transition)?.into())
}

#[pyfunction]
#[pyo3(name = "offer_reconcile_taker_fields")]
fn offer_reconcile_taker_fields_py(
    coinset_confirmed_tx_ids: Vec<String>,
    coinset_mempool_tx_ids: Vec<String>,
    status: Option<i64>,
    current_state: &str,
    next_state: &str,
) -> (String, String) {
    let fields = taker_fields(
        &coinset_confirmed_tx_ids,
        &coinset_mempool_tx_ids,
        status,
        current_state,
        next_state,
    );
    (fields.taker_signal, fields.taker_diagnostic)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(reconciled_state_from_dexie_status_py, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_missing_watched_offer_transition_py, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_watched_offer_transition_py, m)?)?;
    m.add_function(wrap_pyfunction!(offer_reconcile_taker_fields_py, m)?)?;
    Ok(())
}
