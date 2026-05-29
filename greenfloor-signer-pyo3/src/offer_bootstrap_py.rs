use pyo3::prelude::*;
use pyo3::types::PyList;

use crate::py_utils::{
    bootstrap_early_phase_from_py, bootstrap_executed_phase_from_py,
    plan_bootstrap_mixed_outputs_from_py,
};

#[pyfunction]
#[pyo3(name = "plan_bootstrap_mixed_outputs", signature = (*, ladder_entries, spendable_coins))]
fn plan_bootstrap_mixed_outputs_py(
    py: Python<'_>,
    ladder_entries: &Bound<'_, PyList>,
    spendable_coins: &Bound<'_, PyList>,
) -> PyResult<Py<PyAny>> {
    Ok(plan_bootstrap_mixed_outputs_from_py(py, ladder_entries, spendable_coins)?.unbind())
}

#[pyfunction]
#[pyo3(name = "bootstrap_early_phase", signature = (*, outcome_kind, total_output_amount=0))]
fn bootstrap_early_phase_py(
    py: Python<'_>,
    outcome_kind: &str,
    total_output_amount: i64,
) -> PyResult<Option<Py<PyAny>>> {
    Ok(bootstrap_early_phase_from_py(py, outcome_kind, total_output_amount)?.map(Bound::unbind))
}

#[pyfunction]
#[pyo3(name = "bootstrap_executed_phase", signature = (*, remaining_kind, total_output_amount=0))]
fn bootstrap_executed_phase_py(
    py: Python<'_>,
    remaining_kind: &str,
    total_output_amount: i64,
) -> PyResult<Py<PyAny>> {
    Ok(bootstrap_executed_phase_from_py(py, remaining_kind, total_output_amount)?.unbind())
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(plan_bootstrap_mixed_outputs_py, m)?)?;
    m.add_function(wrap_pyfunction!(bootstrap_early_phase_py, m)?)?;
    m.add_function(wrap_pyfunction!(bootstrap_executed_phase_py, m)?)?;
    Ok(())
}
