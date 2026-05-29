use pyo3::prelude::*;
use signer_core::evaluate_low_inventory_alert;

use crate::py_utils::{low_inventory_evaluation_to_py, low_inventory_input_from_py};

#[pyfunction]
#[pyo3(name = "evaluate_low_inventory_alert")]
fn evaluate_low_inventory_alert_py(
    py: Python<'_>,
    input: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let parsed = low_inventory_input_from_py(input)?;
    let evaluation = evaluate_low_inventory_alert(&parsed);
    Ok(low_inventory_evaluation_to_py(py, &evaluation)?.into())
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(evaluate_low_inventory_alert_py, m)?)?;
    Ok(())
}
