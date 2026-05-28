use pyo3::prelude::*;

use signer_core::{
    dedupe_sorted_market_ids, enqueue_immediate_requeue, market_cycle_phases, next_disabled_market_log_deadline,
    select_market_batch, should_log_disabled_market, should_try_cat_inventory_fallback,
    should_use_market_slot_dispatch,
};

use crate::cycle::orchestration_py::market_batch_selection_to_py;

#[pyfunction]
#[pyo3(name = "select_market_batch")]
fn select_market_batch_py(
    enabled_market_ids: Vec<String>,
    slot_count: usize,
    cursor: usize,
    immediate_requeue_ids: Vec<String>,
) -> PyResult<Py<PyAny>> {
    let selection = select_market_batch(
        &enabled_market_ids,
        slot_count,
        cursor,
        &immediate_requeue_ids,
    );
    Python::attach(|py| Ok(market_batch_selection_to_py(py, &selection)?.into()))
}

#[pyfunction]
#[pyo3(name = "enqueue_immediate_requeue")]
fn enqueue_immediate_requeue_py(
    immediate_requeue_ids: Vec<String>,
    market_id: &str,
) -> Vec<String> {
    enqueue_immediate_requeue(&immediate_requeue_ids, market_id)
}

#[pyfunction]
#[pyo3(name = "should_use_market_slot_dispatch")]
fn should_use_market_slot_dispatch_py(enabled_market_count: usize, slot_count: usize) -> bool {
    should_use_market_slot_dispatch(enabled_market_count, slot_count)
}

#[pyfunction]
#[pyo3(name = "dedupe_sorted_market_ids")]
fn dedupe_sorted_market_ids_py(market_ids: Vec<String>) -> Vec<String> {
    dedupe_sorted_market_ids(&market_ids)
}

#[pyfunction]
#[pyo3(name = "should_log_disabled_market")]
fn should_log_disabled_market_py(now_monotonic: f64, next_log_deadline: f64) -> bool {
    should_log_disabled_market(now_monotonic, next_log_deadline)
}

#[pyfunction]
#[pyo3(name = "next_disabled_market_log_deadline")]
fn next_disabled_market_log_deadline_py(now_monotonic: f64, interval_seconds: u64) -> f64 {
    next_disabled_market_log_deadline(now_monotonic, interval_seconds)
}

#[pyfunction]
#[pyo3(name = "should_try_cat_inventory_fallback")]
fn should_try_cat_inventory_fallback_py(coinset_scan_empty: bool, base_asset: &str) -> bool {
    should_try_cat_inventory_fallback(coinset_scan_empty, base_asset)
}

#[pyfunction]
#[pyo3(name = "market_cycle_phases")]
fn market_cycle_phases_py() -> Vec<String> {
    market_cycle_phases()
        .iter()
        .map(|phase| phase.as_str().to_string())
        .collect()
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(select_market_batch_py, m)?)?;
    m.add_function(wrap_pyfunction!(enqueue_immediate_requeue_py, m)?)?;
    m.add_function(wrap_pyfunction!(should_use_market_slot_dispatch_py, m)?)?;
    m.add_function(wrap_pyfunction!(dedupe_sorted_market_ids_py, m)?)?;
    m.add_function(wrap_pyfunction!(should_log_disabled_market_py, m)?)?;
    m.add_function(wrap_pyfunction!(next_disabled_market_log_deadline_py, m)?)?;
    m.add_function(wrap_pyfunction!(should_try_cat_inventory_fallback_py, m)?)?;
    m.add_function(wrap_pyfunction!(market_cycle_phases_py, m)?)?;
    Ok(())
}
