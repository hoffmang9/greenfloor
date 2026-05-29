use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde_json::Value;

use signer_core::{
    bootstrap_block_error, compute_signer_offer_leg_amounts, dexie_offer_asset_expectation_error,
    expected_publish_asset_fields, mojo_multiplier_for_leg, normalize_offer_asset_id,
    normalize_offer_side, quote_mojos_for_base_size, resolve_offer_expiry_for_pricing,
    resolve_quote_price_for_pricing, signer_split_asset_id,
};

use crate::py_utils::{py_any_to_json, request_dict_to_json, to_py_err};

fn pricing_from_py(pricing: &Bound<'_, PyAny>) -> PyResult<Value> {
    if let Ok(dict) = pricing.downcast::<PyDict>() {
        return request_dict_to_json(dict);
    }
    Err(PyValueError::new_err("pricing must be a dict"))
}

#[pyfunction]
#[pyo3(name = "resolve_offer_expiry_for_pricing")]
fn resolve_offer_expiry_for_pricing_py(pricing: &Bound<'_, PyAny>) -> PyResult<(String, i64)> {
    let pricing = pricing_from_py(pricing)?;
    let (unit, value) = resolve_offer_expiry_for_pricing(&pricing);
    Ok((unit.to_string(), value))
}

#[pyfunction]
#[pyo3(name = "resolve_quote_price_for_pricing")]
fn resolve_quote_price_for_pricing_py(pricing: &Bound<'_, PyAny>) -> PyResult<f64> {
    let pricing = pricing_from_py(pricing)?;
    resolve_quote_price_for_pricing(&pricing).map_err(to_py_err)
}

#[pyfunction]
#[pyo3(name = "normalize_offer_side")]
fn normalize_offer_side_py(action_side: &str) -> String {
    normalize_offer_side(action_side).to_string()
}

#[pyfunction]
#[pyo3(name = "quote_mojos_for_base_size")]
fn quote_mojos_for_base_size_py(
    size_base_units: i64,
    quote_price: f64,
    quote_unit_multiplier: i64,
) -> i64 {
    quote_mojos_for_base_size(size_base_units, quote_price, quote_unit_multiplier)
}

#[pyfunction]
#[pyo3(name = "signer_split_asset_id")]
fn signer_split_asset_id_py(
    action_side: &str,
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
) -> String {
    signer_split_asset_id(action_side, resolved_base_asset_id, resolved_quote_asset_id)
}

#[pyfunction]
#[pyo3(name = "normalize_offer_asset_id")]
fn normalize_offer_asset_id_py(asset_id: &str) -> String {
    normalize_offer_asset_id(asset_id)
}

#[pyfunction]
#[pyo3(name = "compute_signer_offer_leg_amounts")]
fn compute_signer_offer_leg_amounts_py(
    size_base_units: i64,
    quote_price: f64,
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
    action_side: &str,
    pricing: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let pricing = pricing_from_py(pricing)?;
    let leg = compute_signer_offer_leg_amounts(
        size_base_units,
        quote_price,
        resolved_base_asset_id,
        resolved_quote_asset_id,
        action_side,
        &pricing,
    )
    .map_err(to_py_err)?;
    Python::attach(|py| {
        let dict = PyDict::new(py);
        dict.set_item("offer_asset_id", leg.offer_asset_id)?;
        dict.set_item("request_asset_id", leg.request_asset_id)?;
        dict.set_item("offer_amount_mojos", leg.offer_amount_mojos)?;
        dict.set_item("request_amount_mojos", leg.request_amount_mojos)?;
        Ok(dict.into())
    })
}

#[pyfunction]
#[pyo3(name = "mojo_multiplier_for_leg")]
fn mojo_multiplier_for_leg_py(
    pricing: &Bound<'_, PyAny>,
    field: &str,
    asset_id: &str,
) -> PyResult<i64> {
    let pricing = pricing_from_py(pricing)?;
    Ok(mojo_multiplier_for_leg(&pricing, field, asset_id))
}

#[pyfunction]
#[pyo3(name = "dexie_offer_asset_expectation_error")]
fn dexie_offer_asset_expectation_error_py(
    offered: &Bound<'_, PyAny>,
    requested: &Bound<'_, PyAny>,
    expected_offered_asset_id: &str,
    expected_offered_symbol: &str,
    expected_requested_asset_id: &str,
    expected_requested_symbol: &str,
) -> PyResult<Option<String>> {
    let offered = py_any_to_json(offered)?;
    let requested = py_any_to_json(requested)?;
    Ok(dexie_offer_asset_expectation_error(
        &offered,
        &requested,
        expected_offered_asset_id,
        expected_offered_symbol,
        expected_requested_asset_id,
        expected_requested_symbol,
    ))
}

#[pyfunction]
#[pyo3(name = "bootstrap_block_error")]
fn bootstrap_block_error_py(
    bootstrap_status: &str,
    bootstrap_reason: &str,
    bootstrap_ready: bool,
) -> Option<String> {
    bootstrap_block_error(bootstrap_status, bootstrap_reason, bootstrap_ready)
}

#[pyfunction]
#[pyo3(name = "expected_publish_asset_fields")]
fn expected_publish_asset_fields_py(
    side: &str,
    base_symbol: &str,
    quote_asset: &str,
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
) -> PyResult<Py<PyAny>> {
    Python::attach(|py| {
        let expected = expected_publish_asset_fields(
            side,
            base_symbol,
            quote_asset,
            resolved_base_asset_id,
            resolved_quote_asset_id,
        );
        let dict = PyDict::new(py);
        dict.set_item(
            "expected_offered_asset_id",
            expected.expected_offered_asset_id,
        )?;
        dict.set_item("expected_offered_symbol", expected.expected_offered_symbol)?;
        dict.set_item(
            "expected_requested_asset_id",
            expected.expected_requested_asset_id,
        )?;
        dict.set_item(
            "expected_requested_symbol",
            expected.expected_requested_symbol,
        )?;
        Ok(dict.into())
    })
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(normalize_offer_side_py, m)?)?;
    m.add_function(wrap_pyfunction!(quote_mojos_for_base_size_py, m)?)?;
    m.add_function(wrap_pyfunction!(signer_split_asset_id_py, m)?)?;
    m.add_function(wrap_pyfunction!(normalize_offer_asset_id_py, m)?)?;
    m.add_function(wrap_pyfunction!(compute_signer_offer_leg_amounts_py, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_offer_expiry_for_pricing_py, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_quote_price_for_pricing_py, m)?)?;
    m.add_function(wrap_pyfunction!(mojo_multiplier_for_leg_py, m)?)?;
    m.add_function(wrap_pyfunction!(dexie_offer_asset_expectation_error_py, m)?)?;
    m.add_function(wrap_pyfunction!(bootstrap_block_error_py, m)?)?;
    m.add_function(wrap_pyfunction!(expected_publish_asset_fields_py, m)?)?;
    Ok(())
}
