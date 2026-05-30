use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use engine_core::daemon::watchlist::{
    active_offer_counts_by_size_and_side_detail, active_offer_counts_by_size_detail,
};
use engine_core::storage::SqliteStore;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};

use crate::py_utils::to_py_err;

fn parse_clock(clock_iso: Option<&str>) -> PyResult<DateTime<Utc>> {
    let Some(raw) = clock_iso.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(Utc::now());
    };
    let normalized = raw.replace('Z', "+00:00");
    if let Ok(parsed) = DateTime::parse_from_rfc3339(&normalized) {
        return Ok(parsed.with_timezone(&Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S") {
        return Ok(naive.and_utc());
    }
    Err(pyo3::exceptions::PyValueError::new_err(format!(
        "invalid clock: {raw}"
    )))
}

fn counts_dict(py: Python<'_>, counts: &BTreeMap<i64, i64>) -> PyResult<Py<PyAny>> {
    let out = PyDict::new(py);
    for (size, count) in counts {
        out.set_item(size, count)?;
    }
    Ok(out.into())
}

fn state_counts_dict(py: Python<'_>, counts: &HashMap<String, i64>) -> PyResult<Py<PyAny>> {
    let out = PyDict::new(py);
    for (state, count) in counts {
        out.set_item(state, count)?;
    }
    Ok(out.into())
}

#[pyfunction]
#[pyo3(signature = (db_path, market_id, *, dexie_size_by_offer_id=None, tracked_sizes=None, clock_iso=None))]
fn active_offer_counts_by_size(
    db_path: PathBuf,
    market_id: String,
    dexie_size_by_offer_id: Option<HashMap<String, i64>>,
    tracked_sizes: Option<Vec<i64>>,
    clock_iso: Option<String>,
) -> PyResult<Py<PyAny>> {
    let store = SqliteStore::open(&db_path).map_err(to_py_err)?;
    let clock = parse_clock(clock_iso.as_deref())?;
    let tracked = tracked_sizes.unwrap_or_default();
    let dexie_ref = dexie_size_by_offer_id.as_ref();
    let (counts, state_counts, unmapped) =
        active_offer_counts_by_size_detail(&store, &market_id, dexie_ref, &tracked, clock)
            .map_err(to_py_err)?;
    Python::attach(|py| {
        let out = PyDict::new(py);
        out.set_item("counts_by_size", counts_dict(py, &counts)?)?;
        out.set_item("state_counts", state_counts_dict(py, &state_counts)?)?;
        out.set_item("unmapped", unmapped)?;
        Ok(out.into())
    })
}

#[pyfunction]
#[pyo3(signature = (db_path, market_id, *, dexie_size_by_offer_id=None, tracked_sizes=None, clock_iso=None))]
fn active_offer_counts_by_size_and_side(
    db_path: PathBuf,
    market_id: String,
    dexie_size_by_offer_id: Option<HashMap<String, i64>>,
    tracked_sizes: Option<Vec<i64>>,
    clock_iso: Option<String>,
) -> PyResult<Py<PyAny>> {
    let store = SqliteStore::open(&db_path).map_err(to_py_err)?;
    let clock = parse_clock(clock_iso.as_deref())?;
    let tracked = tracked_sizes.unwrap_or_default();
    let dexie_ref = dexie_size_by_offer_id.as_ref();
    let (buy_counts, sell_counts, state_counts, unmapped) =
        active_offer_counts_by_size_and_side_detail(&store, &market_id, dexie_ref, &tracked, clock)
            .map_err(to_py_err)?;
    Python::attach(|py| {
        let counts_by_side = PyDict::new(py);
        counts_by_side.set_item("buy", counts_dict(py, &buy_counts)?)?;
        counts_by_side.set_item("sell", counts_dict(py, &sell_counts)?)?;
        let out = PyDict::new(py);
        out.set_item("counts_by_side", counts_by_side)?;
        out.set_item("state_counts", state_counts_dict(py, &state_counts)?)?;
        out.set_item("unmapped", unmapped)?;
        Ok(out.into())
    })
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(active_offer_counts_by_size, m)?)?;
    m.add_function(wrap_pyfunction!(active_offer_counts_by_size_and_side, m)?)?;
    Ok(())
}
