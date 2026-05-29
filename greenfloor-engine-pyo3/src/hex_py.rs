use engine_core::{
    default_mojo_multiplier_for_asset, is_canonical_xch_asset, is_hex_id, normalize_hex_id,
};
use pyo3::prelude::*;

#[pyfunction]
#[pyo3(name = "is_hex_id")]
fn is_hex_id_py(value: &str) -> bool {
    is_hex_id(value)
}

#[pyfunction]
#[pyo3(name = "normalize_hex_id")]
fn normalize_hex_id_py(value: &str) -> String {
    normalize_hex_id(value)
}

#[pyfunction]
#[pyo3(name = "canonical_is_xch")]
fn canonical_is_xch_py(asset_id: &str) -> bool {
    is_canonical_xch_asset(asset_id)
}

#[pyfunction]
#[pyo3(name = "default_mojo_multiplier_for_asset")]
fn default_mojo_multiplier_for_asset_py(asset_id: &str) -> i64 {
    default_mojo_multiplier_for_asset(asset_id)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(is_hex_id_py, m)?)?;
    m.add_function(wrap_pyfunction!(normalize_hex_id_py, m)?)?;
    m.add_function(wrap_pyfunction!(canonical_is_xch_py, m)?)?;
    m.add_function(wrap_pyfunction!(default_mojo_multiplier_for_asset_py, m)?)?;
    Ok(())
}
