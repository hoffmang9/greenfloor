//! PyO3 bindings for the GreenFloor Rust engine (`greenfloor-engine` crate).
//!
//! The extension module is exported as `greenfloor_engine` (ADR 0010). Python
//! callers should import through `greenfloor.core.engine_bridge.import_engine`.

mod coin_ops_py;
mod cycle;
mod daemon_py;
mod engine_contracts_py;
mod execution_py;
mod hex_py;
mod manager_py;
mod notifications_py;
mod offer_action_py;
mod offer_bootstrap_py;
mod offer_build_py;
mod offer_request_py;
mod py_utils;
mod retry_py;
mod strategy_py;
mod wallet_io_py;
mod watchlist_py;

use std::path::Path;
use std::sync::OnceLock;

use engine_core::{
    build_and_optionally_broadcast_vault_cat_mixed_split, build_vault_cat_offer,
    encode_offer_from_spend_bundle_bytes, from_input_spend_bundle_bytes,
    from_input_spend_bundle_xch_bytes, get_conservative_fee_estimate, get_fee_estimate,
    load_signer_config, push_tx_hex, resolve_offer_assets_via_coinset, resolve_vault_context,
    validate_offer_structure, validate_offer_text, verify_offer_for_dexie, CreateOfferRequest,
    MixedSplitRequest,
};
use pyo3::prelude::*;

use py_utils::{dict_from_json_value, request_dict_to_json, to_py_err};
use pyo3::types::{PyDict, PyModule, PyTuple};

pub(crate) fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

#[pyfunction]
#[pyo3(name = "resolve_vault_context")]
fn resolve_vault_context_py(config_path: &str) -> PyResult<Py<PyAny>> {
    let config = load_signer_config(Path::new(config_path)).map_err(to_py_err)?;
    let context = runtime()
        .block_on(resolve_vault_context(config))
        .map_err(to_py_err)?;
    Python::attach(|py| {
        dict_from_json_value(py, serde_json::to_value(&context).map_err(to_py_err)?)
    })
}

#[pyfunction]
#[pyo3(name = "build_vault_cat_offer")]
fn build_vault_cat_offer_py(config_path: &str, request: &Bound<'_, PyDict>) -> PyResult<Py<PyAny>> {
    let config = load_signer_config(Path::new(config_path)).map_err(to_py_err)?;
    let payload = request_dict_to_json(request)?;
    let offer_request: CreateOfferRequest = serde_json::from_value(payload).map_err(to_py_err)?;
    let result = runtime()
        .block_on(build_vault_cat_offer(config, offer_request))
        .map_err(to_py_err)?;
    Python::attach(|py| dict_from_json_value(py, serde_json::to_value(&result).map_err(to_py_err)?))
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
    let split_request: MixedSplitRequest = serde_json::from_value(payload).map_err(to_py_err)?;
    let result = runtime()
        .block_on(build_and_optionally_broadcast_vault_cat_mixed_split(
            config,
            split_request,
            broadcast,
        ))
        .map_err(to_py_err)?;
    Python::attach(|py| dict_from_json_value(py, serde_json::to_value(&result).map_err(to_py_err)?))
}

fn resolve_offer_assets_via_coinset_py_impl(
    config_path: &str,
    base_asset: &str,
    quote_asset: &str,
) -> PyResult<Py<PyAny>> {
    let config = load_signer_config(Path::new(config_path)).map_err(to_py_err)?;
    let (base, quote) = runtime()
        .block_on(resolve_offer_assets_via_coinset(
            config,
            base_asset,
            quote_asset,
        ))
        .map_err(to_py_err)?;
    Python::attach(|py| Ok(PyTuple::new(py, [base, quote])?.into()))
}

#[pyfunction]
#[pyo3(name = "resolve_offer_assets_via_coinset")]
fn resolve_offer_assets_via_coinset_py(
    config_path: &str,
    base_asset: &str,
    quote_asset: &str,
) -> PyResult<Py<PyAny>> {
    resolve_offer_assets_via_coinset_py_impl(config_path, base_asset, quote_asset)
}

#[pyfunction]
#[pyo3(name = "resolve_offer_asset_ids")]
fn resolve_offer_asset_ids_py(
    config_path: &str,
    base_asset: &str,
    quote_asset: &str,
) -> PyResult<Py<PyAny>> {
    resolve_offer_assets_via_coinset_py_impl(config_path, base_asset, quote_asset)
}

/// Full Dexie pre-post validation (structure, expiry, duplicate spends).
/// Prefer :func:`verify_offer_for_dexie` when callers need stable error code strings.
#[pyfunction]
#[pyo3(name = "validate_offer")]
fn validate_offer_py(offer: &str) -> PyResult<()> {
    validate_offer_text(offer).map_err(to_py_err)
}

#[pyfunction]
#[pyo3(name = "validate_offer_structure")]
fn validate_offer_structure_py(offer: &str) -> PyResult<()> {
    validate_offer_structure(offer).map_err(to_py_err)
}

#[pyfunction]
#[pyo3(name = "verify_offer_for_dexie")]
fn verify_offer_for_dexie_py(offer: &str) -> Option<String> {
    verify_offer_for_dexie(offer)
}

#[pyfunction]
#[pyo3(name = "encode_offer")]
fn encode_offer_py(spend_bundle_bytes: &[u8]) -> PyResult<String> {
    encode_offer_from_spend_bundle_bytes(spend_bundle_bytes).map_err(to_py_err)
}

#[pyfunction]
#[pyo3(name = "from_input_spend_bundle")]
fn from_input_spend_bundle_py(
    spend_bundle_bytes: &[u8],
    requested_payments_xch: Vec<(Vec<u8>, Vec<(Vec<u8>, u64)>)>,
    requested_payments_cats: Vec<(Vec<u8>, Vec<u8>, Vec<(Vec<u8>, u64)>)>,
) -> PyResult<Vec<u8>> {
    from_input_spend_bundle_bytes(
        spend_bundle_bytes,
        requested_payments_xch,
        requested_payments_cats,
    )
    .map_err(to_py_err)
}

#[pyfunction]
#[pyo3(name = "from_input_spend_bundle_xch")]
fn from_input_spend_bundle_xch_py(
    spend_bundle_bytes: &[u8],
    requested_payments_xch: Vec<(Vec<u8>, Vec<(Vec<u8>, u64)>)>,
) -> PyResult<Vec<u8>> {
    from_input_spend_bundle_xch_bytes(spend_bundle_bytes, requested_payments_xch).map_err(to_py_err)
}

#[pyfunction]
#[pyo3(name = "coinset_push_tx")]
fn coinset_push_tx_py(
    network: &str,
    base_url: &str,
    spend_bundle_hex: &str,
) -> PyResult<Py<PyAny>> {
    let base = base_url.trim();
    let base_opt = if base.is_empty() { None } else { Some(base) };
    let payload = runtime()
        .block_on(push_tx_hex(network, base_opt, spend_bundle_hex))
        .map_err(to_py_err)?;
    Python::attach(|py| dict_from_json_value(py, payload))
}

#[pyfunction]
#[pyo3(name = "coinset_get_fee_estimate")]
fn coinset_get_fee_estimate_py(
    network: &str,
    base_url: &str,
    target_times: Vec<u64>,
    cost: u64,
    spend_count: Option<u64>,
) -> PyResult<Py<PyAny>> {
    let base = base_url.trim();
    let base_opt = if base.is_empty() { None } else { Some(base) };
    let payload = runtime()
        .block_on(get_fee_estimate(
            network,
            base_opt,
            target_times,
            cost,
            spend_count,
        ))
        .map_err(to_py_err)?;
    Python::attach(|py| dict_from_json_value(py, payload))
}

#[pyfunction]
#[pyo3(name = "coinset_get_conservative_fee_estimate")]
fn coinset_get_conservative_fee_estimate_py(
    network: &str,
    base_url: &str,
    cost: u64,
    spend_count: Option<u64>,
) -> PyResult<Option<u64>> {
    let base = base_url.trim();
    let base_opt = if base.is_empty() { None } else { Some(base) };
    runtime()
        .block_on(get_conservative_fee_estimate(
            network,
            base_opt,
            cost,
            spend_count,
        ))
        .map_err(to_py_err)
}

#[pymodule]
fn greenfloor_engine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(resolve_vault_context_py, m)?)?;
    m.add_function(wrap_pyfunction!(build_vault_cat_offer_py, m)?)?;
    m.add_function(wrap_pyfunction!(build_mixed_split_py, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_offer_assets_via_coinset_py, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_offer_asset_ids_py, m)?)?;
    m.add_function(wrap_pyfunction!(validate_offer_py, m)?)?;
    m.add_function(wrap_pyfunction!(validate_offer_structure_py, m)?)?;
    m.add_function(wrap_pyfunction!(verify_offer_for_dexie_py, m)?)?;
    m.add_function(wrap_pyfunction!(encode_offer_py, m)?)?;
    m.add_function(wrap_pyfunction!(from_input_spend_bundle_py, m)?)?;
    m.add_function(wrap_pyfunction!(from_input_spend_bundle_xch_py, m)?)?;
    m.add_function(wrap_pyfunction!(coinset_push_tx_py, m)?)?;
    m.add_function(wrap_pyfunction!(coinset_get_fee_estimate_py, m)?)?;
    m.add_function(wrap_pyfunction!(
        coinset_get_conservative_fee_estimate_py,
        m
    )?)?;
    offer_action_py::register(m)?;
    coin_ops_py::register(m)?;
    cycle::register(m)?;
    hex_py::register(m)?;
    notifications_py::register(m)?;
    offer_bootstrap_py::register(m)?;
    offer_build_py::register(m)?;
    offer_request_py::register(m)?;
    retry_py::register(m)?;
    wallet_io_py::register(m)?;
    engine_contracts_py::register(m)?;
    manager_py::register(m)?;
    daemon_py::register(m)?;
    watchlist_py::register(m)?;
    Ok(())
}
