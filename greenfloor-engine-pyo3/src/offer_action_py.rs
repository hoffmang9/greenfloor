use std::path::Path;

use engine_core::config::SignerConfig;
use engine_core::offer::action::{
    build_bls_offer_for_action, build_signer_offer_for_action, try_normalize_resolved_assets,
    BuildOfferForActionRequest,
};
use engine_core::{load_bls_master_secret_key, load_signer_config, Error};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule, PyTuple};

use crate::py_utils::{dict_from_json_value, request_dict_to_json, to_py_err};
use crate::{block_on_engine, parse_master_sk_bytes, runtime};

fn asset_pair_to_py_tuple(py: Python<'_>, base: String, quote: String) -> PyResult<Py<PyAny>> {
    Ok(PyTuple::new(py, [base, quote])?.into())
}

fn optional_signer_config(config_path: Option<&str>) -> PyResult<Option<SignerConfig>> {
    let Some(path) = config_path.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    load_signer_config(Path::new(path))
        .map(Some)
        .map_err(to_py_err)
}

#[pyfunction]
#[pyo3(name = "try_normalize_offer_asset_ids")]
fn try_normalize_offer_asset_ids_py(base_asset: &str, quote_asset: &str) -> PyResult<Py<PyAny>> {
    match try_normalize_resolved_assets(base_asset, quote_asset) {
        Ok((base, quote)) => Python::attach(|py| asset_pair_to_py_tuple(py, base, quote)),
        Err(Error::ResolvedAssetsCollideForNonXchPair) => {
            Err(to_py_err(Error::ResolvedAssetsCollideForNonXchPair))
        }
        Err(_) => Python::attach(|py| Ok(py.None())),
    }
}

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
#[pyo3(signature = (network, key_id, request, *, config_path=None))]
#[pyo3(name = "build_bls_offer_for_action_key")]
fn build_bls_offer_for_action_key_py(
    network: &str,
    key_id: &str,
    request: &Bound<'_, PyDict>,
    config_path: Option<&str>,
) -> PyResult<Py<PyAny>> {
    let master_sk = load_bls_master_secret_key(key_id.trim()).map_err(to_py_err)?;
    let config = optional_signer_config(config_path)?;
    let payload = request_dict_to_json(request)?;
    let offer_request: BuildOfferForActionRequest =
        serde_json::from_value(payload).map_err(to_py_err)?;
    let result = block_on_engine(build_bls_offer_for_action(
        network,
        &master_sk,
        config.as_ref(),
        offer_request,
    ))
    .map_err(to_py_err)?;
    Python::attach(|py| dict_from_json_value(py, serde_json::to_value(&result).map_err(to_py_err)?))
}

/// Internal/test entry: build a BLS action offer from raw master secret key bytes.
#[pyfunction]
#[pyo3(signature = (network, master_sk_bytes, request, *, config_path=None))]
#[pyo3(name = "build_bls_offer_for_action_sk")]
fn build_bls_offer_for_action_sk_py(
    network: &str,
    master_sk_bytes: &[u8],
    request: &Bound<'_, PyDict>,
    config_path: Option<&str>,
) -> PyResult<Py<PyAny>> {
    let master_sk = parse_master_sk_bytes(master_sk_bytes)?;
    let config = optional_signer_config(config_path)?;
    let payload = request_dict_to_json(request)?;
    let offer_request: BuildOfferForActionRequest =
        serde_json::from_value(payload).map_err(to_py_err)?;
    let result = block_on_engine(build_bls_offer_for_action(
        network,
        &master_sk,
        config.as_ref(),
        offer_request,
    ))
    .map_err(to_py_err)?;
    Python::attach(|py| dict_from_json_value(py, serde_json::to_value(&result).map_err(to_py_err)?))
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(try_normalize_offer_asset_ids_py, m)?)?;
    m.add_function(wrap_pyfunction!(build_signer_offer_for_action_py, m)?)?;
    m.add_function(wrap_pyfunction!(build_bls_offer_for_action_key_py, m)?)?;
    m.add_function(wrap_pyfunction!(build_bls_offer_for_action_sk_py, m)?)?;
    Ok(())
}
