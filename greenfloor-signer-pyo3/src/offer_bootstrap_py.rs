use pyo3::prelude::*;
use pyo3::types::PyList;

use crate::py_utils::plan_bootstrap_mixed_outputs_from_py;

#[pyfunction]
#[pyo3(name = "plan_bootstrap_mixed_outputs", signature = (*, ladder_entries, spendable_coins))]
fn plan_bootstrap_mixed_outputs_py(
    py: Python<'_>,
    ladder_entries: &Bound<'_, PyList>,
    spendable_coins: &Bound<'_, PyList>,
) -> PyResult<Py<PyAny>> {
    Ok(plan_bootstrap_mixed_outputs_from_py(py, ladder_entries, spendable_coins)?.unbind())
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(plan_bootstrap_mixed_outputs_py, m)?)?;
    Ok(())
}
