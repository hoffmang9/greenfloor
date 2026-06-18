use pyo3::prelude::*;

use engine_core::cycle::{needs_inventory_fallback, resolve_inventory_scan_source};

#[pyfunction]
#[pyo3(name = "needs_inventory_fallback")]
fn needs_inventory_fallback_py(bucket_counts_available: bool, coinset_scan_empty: bool) -> bool {
    needs_inventory_fallback(bucket_counts_available, coinset_scan_empty)
}

#[pyfunction]
#[pyo3(name = "resolve_inventory_scan_source")]
fn resolve_inventory_scan_source_py(
    coinset_scan_found_coins: bool,
    coinset_scan_empty: bool,
    cat_scan_found_coins: bool,
    wallet_scan_found_coins: bool,
) -> &'static str {
    resolve_inventory_scan_source(
        coinset_scan_found_coins,
        coinset_scan_empty,
        cat_scan_found_coins,
        wallet_scan_found_coins,
    )
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(needs_inventory_fallback_py, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_inventory_scan_source_py, m)?)?;
    Ok(())
}
