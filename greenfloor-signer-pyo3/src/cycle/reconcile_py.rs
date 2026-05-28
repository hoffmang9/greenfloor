use pyo3::prelude::*;
use pyo3::types::PyDict;

use signer_core::{
    resolve_missing_watched_offer_transition, resolve_watched_offer_transition_from_signals,
    unchanged_offer_transition, unsupported_venue_offer_transition, CycleOfferTransition,
};

use crate::py_utils::{cycle_offer_transition_class, to_py_err};

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
    kwargs.set_item("taker_signal", &transition.taker_signal)?;
    kwargs.set_item("taker_diagnostic", &transition.taker_diagnostic)?;
    kwargs.set_item("coinset_tx_ids", &transition.coinset_tx_ids)?;
    kwargs.set_item(
        "coinset_confirmed_tx_ids",
        &transition.coinset_confirmed_tx_ids,
    )?;
    kwargs.set_item("coinset_mempool_tx_ids", &transition.coinset_mempool_tx_ids)?;
    cls.call((), Some(&kwargs))
}

#[pyfunction]
#[pyo3(name = "resolve_missing_watched_offer_transition")]
fn resolve_missing_watched_offer_transition_py(
    py: Python<'_>,
    current_state: &str,
) -> PyResult<Py<PyAny>> {
    let transition = resolve_missing_watched_offer_transition(current_state).map_err(to_py_err)?;
    Ok(cycle_offer_transition_to_py(py, &transition)?.into())
}

#[pyfunction]
#[pyo3(name = "resolve_watched_offer_transition_from_signals")]
fn resolve_watched_offer_transition_from_signals_py(
    py: Python<'_>,
    current_state: &str,
    status: Option<i64>,
    coinset_tx_ids: Vec<String>,
    coinset_confirmed_tx_ids: Vec<String>,
    coinset_mempool_tx_ids: Vec<String>,
) -> PyResult<Py<PyAny>> {
    let transition = resolve_watched_offer_transition_from_signals(
        current_state,
        status,
        coinset_tx_ids,
        coinset_confirmed_tx_ids,
        coinset_mempool_tx_ids,
    )
    .map_err(to_py_err)?;
    Ok(cycle_offer_transition_to_py(py, &transition)?.into())
}

#[pyfunction]
#[pyo3(name = "unchanged_offer_transition")]
fn unchanged_offer_transition_py(
    py: Python<'_>,
    current_state: &str,
    reason: &str,
) -> PyResult<Py<PyAny>> {
    let transition = unchanged_offer_transition(current_state, reason).map_err(to_py_err)?;
    Ok(cycle_offer_transition_to_py(py, &transition)?.into())
}

#[pyfunction]
#[pyo3(name = "unsupported_venue_offer_transition")]
fn unsupported_venue_offer_transition_py(
    py: Python<'_>,
    current_state: &str,
    venue: &str,
) -> PyResult<Py<PyAny>> {
    let transition = unsupported_venue_offer_transition(current_state, venue).map_err(to_py_err)?;
    Ok(cycle_offer_transition_to_py(py, &transition)?.into())
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(
        resolve_missing_watched_offer_transition_py,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        resolve_watched_offer_transition_from_signals_py,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(unchanged_offer_transition_py, m)?)?;
    m.add_function(wrap_pyfunction!(unsupported_venue_offer_transition_py, m)?)?;
    Ok(())
}
