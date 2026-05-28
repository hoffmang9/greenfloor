use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use crate::py_utils::{dict_from_json_value, request_dict_to_json, to_py_err};

use signer_core::{
    classify_dexie_stale_offer_status, collect_stale_sweep_candidates, is_dexie_offer_missing_error_text,
    record_stale_sweep_check, OfferStateRow, StaleSweepProgress,
};

#[pyfunction]
#[pyo3(name = "collect_stale_sweep_candidates")]
fn collect_stale_sweep_candidates_py(
    rows: &Bound<'_, PyList>,
    enabled_market_ids: Vec<String>,
    per_market_limit: usize,
) -> PyResult<Py<PyAny>> {
    let mut offer_rows = Vec::new();
    for item in rows.iter() {
        let dict = item.cast::<PyDict>()?;
        let payload = request_dict_to_json(&dict)?;
        offer_rows.push(serde_json::from_value::<OfferStateRow>(payload).map_err(to_py_err)?);
    }
    let candidates =
        collect_stale_sweep_candidates(&offer_rows, &enabled_market_ids, per_market_limit);
    Python::attach(|py| {
        dict_from_json_value(py, serde_json::to_value(candidates).map_err(to_py_err)?)
    })
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
    progress: &Bound<'_, PyDict>,
    hit: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    let progress_json = request_dict_to_json(progress)?;
    let mut current: StaleSweepProgress =
        serde_json::from_value(progress_json).map_err(to_py_err)?;
    let hit_value = if let Some(hit_dict) = hit {
        let hit_json = request_dict_to_json(hit_dict)?;
        Some(serde_json::from_value(hit_json).map_err(to_py_err)?)
    } else {
        None
    };
    current = record_stale_sweep_check(&current, hit_value);
    Python::attach(|py| {
        dict_from_json_value(py, serde_json::to_value(&current).map_err(to_py_err)?)
    })
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(collect_stale_sweep_candidates_py, m)?)?;
    m.add_function(wrap_pyfunction!(classify_dexie_stale_offer_status_py, m)?)?;
    m.add_function(wrap_pyfunction!(is_dexie_offer_missing_error_text_py, m)?)?;
    m.add_function(wrap_pyfunction!(record_stale_sweep_check_py, m)?)?;
    Ok(())
}
