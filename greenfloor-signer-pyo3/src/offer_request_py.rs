use pyo3::prelude::*;

use signer_core::{
    compute_signer_offer_leg_amounts, normalize_offer_asset_id, quote_mojos_for_base_size,
    signer_split_asset_id,
};

use crate::py_utils::{pricing_dict_from_py, signer_offer_leg_amounts_to_py, to_py_err};

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
    py: Python<'_>,
    size_base_units: i64,
    quote_price: f64,
    resolved_base_asset_id: &str,
    resolved_quote_asset_id: &str,
    action_side: &str,
    pricing: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let pricing = pricing_dict_from_py(pricing)?;
    let leg = compute_signer_offer_leg_amounts(
        size_base_units,
        quote_price,
        resolved_base_asset_id,
        resolved_quote_asset_id,
        action_side,
        &pricing,
    )
    .map_err(to_py_err)?;
    Ok(signer_offer_leg_amounts_to_py(py, &leg)?.unbind())
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(quote_mojos_for_base_size_py, m)?)?;
    m.add_function(wrap_pyfunction!(signer_split_asset_id_py, m)?)?;
    m.add_function(wrap_pyfunction!(normalize_offer_asset_id_py, m)?)?;
    m.add_function(wrap_pyfunction!(compute_signer_offer_leg_amounts_py, m)?)?;
    Ok(())
}
