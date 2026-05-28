use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde_json::Value;

use signer_core::{
    mojo_multiplier_for_leg, resolve_offer_expiry_for_pricing, resolve_quote_price_for_pricing,
};

use crate::py_utils::{request_dict_to_json, to_py_err};

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
#[pyo3(name = "mojo_multiplier_for_leg")]
fn mojo_multiplier_for_leg_py(pricing: &Bound<'_, PyAny>, field: &str, asset_id: &str) -> PyResult<i64> {
    let pricing = pricing_from_py(pricing)?;
    Ok(mojo_multiplier_for_leg(&pricing, field, asset_id))
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(resolve_offer_expiry_for_pricing_py, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_quote_price_for_pricing_py, m)?)?;
    m.add_function(wrap_pyfunction!(mojo_multiplier_for_leg_py, m)?)?;
    Ok(())
}
