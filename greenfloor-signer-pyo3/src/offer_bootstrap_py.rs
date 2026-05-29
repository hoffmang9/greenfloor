use pyo3::prelude::*;
use pyo3::types::{PyAny, PyList};

use crate::py_utils::plan_bootstrap_mixed_outputs_from_py;

#[pyfunction]
#[pyo3(name = "plan_bootstrap_mixed_outputs", signature = (*, sell_ladder, spendable_coins))]
fn plan_bootstrap_mixed_outputs_py(
    py: Python<'_>,
    sell_ladder: &Bound<'_, PyList>,
    spendable_coins: &Bound<'_, PyList>,
) -> PyResult<Option<Py<PyAny>>> {
    let plan = plan_bootstrap_mixed_outputs_from_py(py, sell_ladder, spendable_coins)?;
    Ok(plan.map(Bound::unbind))
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(plan_bootstrap_mixed_outputs_py, m)?)?;
    Ok(())
}
