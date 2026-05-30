use pyo3::prelude::*;
use pyo3::types::PyModule;

use crate::engine_contracts_py::{
    build_and_post_offer_typed, PyBuildAndPostOfferRequest, PyBuildAndPostOfferResponse,
};

#[pyfunction]
#[pyo3(name = "build_and_post_offer", signature = (request, /))]
fn build_and_post_offer_py(
    py: Python<'_>,
    request: PyBuildAndPostOfferRequest,
) -> PyResult<Py<PyBuildAndPostOfferResponse>> {
    build_and_post_offer_typed(py, request)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(build_and_post_offer_py, m)?)?;
    Ok(())
}
