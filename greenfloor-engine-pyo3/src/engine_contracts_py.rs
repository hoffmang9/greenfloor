//! PyO3 manager entrypoints using serde JSON request/response boundaries.

use engine_core::manager::{build_and_post_offer, BuildAndPostOfferRequest};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};

use crate::py_utils::{dict_from_json_value, request_dict_to_json, to_py_err};
use crate::runtime;

pub(crate) fn build_and_post_offer_typed(
    py: Python<'_>,
    request: &Bound<'_, PyDict>,
) -> PyResult<Py<PyAny>> {
    let payload = request_dict_to_json(request)?;
    let engine_request: BuildAndPostOfferRequest =
        serde_json::from_value(payload).map_err(to_py_err)?;
    let response = py.detach(move || {
        runtime()
            .block_on(build_and_post_offer(engine_request))
            .map_err(to_py_err)
    })?;
    Python::attach(|py| {
        let out = PyDict::new(py);
        out.set_item("exit_code", response.exit_code)?;
        out.set_item("output", response.output)?;
        out.set_item("payload", dict_from_json_value(py, response.payload)?)?;
        Ok(out.into())
    })
}

pub fn register(_m: &Bound<'_, PyModule>) -> PyResult<()> {
    Ok(())
}
