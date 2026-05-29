use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use signer_core::offer::action::{
    build_bls_offer_for_action, build_signer_offer_for_action, BuildOfferForActionRequest,
};
use signer_core::{load_bls_master_secret_key, load_signer_config};

use crate::py_utils::{dict_from_json_value, request_dict_to_json, to_py_err};
use crate::{block_on_signer, parse_master_sk_bytes, runtime};

#[pyfunction]
#[pyo3(name = "build_signer_offer_for_action")]
fn build_signer_offer_for_action_py(
    config_path: &str,
    request: &Bound<'_, PyDict>,
) -> PyResult<Py<PyAny>> {
    let config = load_signer_config(std::path::Path::new(config_path)).map_err(to_py_err)?;
    let payload = request_dict_to_json(request)?;
    let offer_request: BuildOfferForActionRequest =
        serde_json::from_value(payload).map_err(to_py_err)?;
    let result = runtime()
        .block_on(build_signer_offer_for_action(config, offer_request))
        .map_err(to_py_err)?;
    Python::attach(|py| dict_from_json_value(py, serde_json::to_value(&result).map_err(to_py_err)?))
}

#[pyfunction]
#[pyo3(name = "build_bls_offer_for_action_key")]
fn build_bls_offer_for_action_key_py(
    network: &str,
    key_id: &str,
    request: &Bound<'_, PyDict>,
) -> PyResult<Py<PyAny>> {
    let master_sk = load_bls_master_secret_key(key_id.trim()).map_err(to_py_err)?;
    let payload = request_dict_to_json(request)?;
    let offer_request: BuildOfferForActionRequest =
        serde_json::from_value(payload).map_err(to_py_err)?;
    let result = block_on_signer(build_bls_offer_for_action(
        network,
        &master_sk,
        offer_request,
    ))
    .map_err(to_py_err)?;
    Python::attach(|py| dict_from_json_value(py, serde_json::to_value(&result).map_err(to_py_err)?))
}

/// Internal/test entry: build a BLS action offer from raw master secret key bytes.
#[pyfunction]
#[pyo3(name = "build_bls_offer_for_action_sk")]
fn build_bls_offer_for_action_sk_py(
    network: &str,
    master_sk_bytes: &[u8],
    request: &Bound<'_, PyDict>,
) -> PyResult<Py<PyAny>> {
    let master_sk = parse_master_sk_bytes(master_sk_bytes)?;
    let payload = request_dict_to_json(request)?;
    let offer_request: BuildOfferForActionRequest =
        serde_json::from_value(payload).map_err(to_py_err)?;
    let result = block_on_signer(build_bls_offer_for_action(
        network,
        &master_sk,
        offer_request,
    ))
    .map_err(to_py_err)?;
    Python::attach(|py| dict_from_json_value(py, serde_json::to_value(&result).map_err(to_py_err)?))
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(build_signer_offer_for_action_py, m)?)?;
    m.add_function(wrap_pyfunction!(build_bls_offer_for_action_key_py, m)?)?;
    m.add_function(wrap_pyfunction!(build_bls_offer_for_action_sk_py, m)?)?;
    Ok(())
}
