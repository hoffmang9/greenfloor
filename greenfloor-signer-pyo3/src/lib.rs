extern crate greenfloor_signer as signer_core;

use std::path::Path;
use std::sync::OnceLock;

use signer_core::{
    build_and_optionally_broadcast_vault_cat_mixed_split, build_vault_cat_offer,
    load_signer_config, resolve_offer_asset_ids, resolve_vault_context, CreateOfferRequest,
    MixedSplitRequest,
};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

fn to_py_err<E: std::fmt::Display>(err: E) -> PyErr {
    PyValueError::new_err(err.to_string())
}

fn dict_from_json_value(py: Python<'_>, value: serde_json::Value) -> PyResult<Py<PyAny>> {
    let json = serde_json::to_string(&value).map_err(to_py_err)?;
    let builtins = py.import("json")?;
    let loads = builtins.getattr("loads")?;
    let obj = loads.call1((json,))?;
    Ok(obj.unbind())
}

fn request_dict_to_json(request: &Bound<'_, PyDict>) -> PyResult<serde_json::Value> {
    let py = request.py();
    let json_mod = py.import("json")?;
    let dumps = json_mod.getattr("dumps")?;
    let raw = dumps.call1((request,))?;
    let raw_str: String = raw.extract()?;
    serde_json::from_str(&raw_str).map_err(to_py_err)
}

#[pyfunction]
#[pyo3(name = "resolve_vault_context")]
fn resolve_vault_context_py(config_path: &str) -> PyResult<Py<PyAny>> {
    let config = load_signer_config(Path::new(config_path)).map_err(to_py_err)?;
    let context = runtime()
        .block_on(resolve_vault_context(config))
        .map_err(to_py_err)?;
    Python::attach(|py| {
        dict_from_json_value(
            py,
            serde_json::to_value(&context).map_err(to_py_err)?,
        )
    })
}

#[pyfunction]
#[pyo3(name = "build_vault_cat_offer")]
fn build_vault_cat_offer_py(config_path: &str, request: &Bound<'_, PyDict>) -> PyResult<Py<PyAny>> {
    let config = load_signer_config(Path::new(config_path)).map_err(to_py_err)?;
    let payload = request_dict_to_json(request)?;
    let offer_request: CreateOfferRequest =
        serde_json::from_value(payload).map_err(to_py_err)?;
    let result = runtime()
        .block_on(build_vault_cat_offer(config, offer_request))
        .map_err(to_py_err)?;
    Python::attach(|py| {
        dict_from_json_value(
            py,
            serde_json::to_value(&result).map_err(to_py_err)?,
        )
    })
}

#[pyfunction]
#[pyo3(name = "build_mixed_split")]
fn build_mixed_split_py(config_path: &str, request: &Bound<'_, PyDict>) -> PyResult<Py<PyAny>> {
    let config = load_signer_config(Path::new(config_path)).map_err(to_py_err)?;
    let payload = request_dict_to_json(request)?;
    let broadcast = payload
        .get("broadcast")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let split_request: MixedSplitRequest =
        serde_json::from_value(payload).map_err(to_py_err)?;
    let result = runtime()
        .block_on(build_and_optionally_broadcast_vault_cat_mixed_split(
            config,
            split_request,
            broadcast,
        ))
        .map_err(to_py_err)?;
    Python::attach(|py| {
        dict_from_json_value(
            py,
            serde_json::to_value(&result).map_err(to_py_err)?,
        )
    })
}

#[pyfunction]
#[pyo3(name = "resolve_offer_asset_ids")]
fn resolve_offer_asset_ids_py(
    config_path: &str,
    base_asset: &str,
    quote_asset: &str,
) -> PyResult<Py<PyAny>> {
    let config = load_signer_config(Path::new(config_path)).map_err(to_py_err)?;
    let (base, quote) = runtime()
        .block_on(resolve_offer_asset_ids(config, base_asset, quote_asset))
        .map_err(to_py_err)?;
    Python::attach(|py| {
        let dict = PyDict::new(py);
        dict.set_item("base_asset_id", base)?;
        dict.set_item("quote_asset_id", quote)?;
        Ok(dict.into())
    })
}

#[pymodule]
fn greenfloor_signer(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(resolve_vault_context_py, m)?)?;
    m.add_function(wrap_pyfunction!(build_vault_cat_offer_py, m)?)?;
    m.add_function(wrap_pyfunction!(build_mixed_split_py, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_offer_asset_ids_py, m)?)?;
    Ok(())
}
