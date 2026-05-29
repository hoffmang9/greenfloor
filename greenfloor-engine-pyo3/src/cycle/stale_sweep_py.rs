use pyo3::prelude::*;
use pyo3::types::PyList;

use engine_core::{
    classify_dexie_stale_offer_status, collect_stale_sweep_candidates,
    is_dexie_offer_missing_error_text, record_stale_sweep_check,
};

use crate::cycle::orchestration_py::{
    offer_state_row_from_py, stale_sweep_candidates_to_py_list, stale_sweep_hit_from_py,
    stale_sweep_progress_from_py, stale_sweep_progress_to_py,
};

#[pyfunction]
#[pyo3(name = "collect_stale_sweep_candidates")]
fn collect_stale_sweep_candidates_py(
    rows: &Bound<'_, PyList>,
    enabled_market_ids: Vec<String>,
    per_market_limit: usize,
) -> PyResult<Py<PyAny>> {
    let mut offer_rows = Vec::with_capacity(rows.len());
    for item in rows.iter() {
        offer_rows.push(offer_state_row_from_py(&item)?);
    }
    let candidates =
        collect_stale_sweep_candidates(&offer_rows, &enabled_market_ids, per_market_limit);
    Python::attach(|py| stale_sweep_candidates_to_py_list(py, &candidates))
}

#[pyfunction]
#[pyo3(name = "classify_dexie_stale_offer_status")]
fn classify_dexie_stale_offer_status_py(status: i64) -> Option<String> {
    classify_dexie_stale_offer_status(status).map(str::to_string)
}

#[pyfunction]
#[pyo3(name = "is_dexie_offer_missing_error_text")]
fn is_dexie_offer_missing_error_text_py(error_text: &str) -> bool {
    is_dexie_offer_missing_error_text(error_text)
}

#[pyfunction]
#[pyo3(name = "record_stale_sweep_check")]
fn record_stale_sweep_check_py(
    progress: &Bound<'_, PyAny>,
    hit: Option<&Bound<'_, PyAny>>,
) -> PyResult<Py<PyAny>> {
    let mut current = stale_sweep_progress_from_py(progress)?;
    let hit_value = hit.map(stale_sweep_hit_from_py).transpose()?;
    current = record_stale_sweep_check(&current, hit_value);
    Python::attach(|py| Ok(stale_sweep_progress_to_py(py, &current)?.into()))
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(collect_stale_sweep_candidates_py, m)?)?;
    m.add_function(wrap_pyfunction!(classify_dexie_stale_offer_status_py, m)?)?;
    m.add_function(wrap_pyfunction!(is_dexie_offer_missing_error_text_py, m)?)?;
    m.add_function(wrap_pyfunction!(record_stale_sweep_check_py, m)?)?;
    Ok(())
}
