use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use engine_core::daemon::{
    build_dexie_size_by_offer_id,
    watchlist::{
        active_offer_counts_by_size_and_side_detail, active_offer_counts_by_size_detail,
        match_watched_coin_ids, set_watched_coin_ids_for_market,
        time::RESEED_MEMPOOL_MAX_AGE_SECONDS, update_market_coin_watchlist_from_offers,
        watched_coin_ids_for_market, watchlist_offer_ids, CoinWatchlistCache,
    },
};
use engine_core::storage::SqliteStore;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyModule};
use serde_json::Value;

use crate::py_utils::{py_any_to_json, to_py_err};

#[pyclass(name = "CoinWatchlistCache")]
#[derive(Clone)]
pub(crate) struct PyCoinWatchlistCache {
    pub(crate) inner: Arc<CoinWatchlistCache>,
}

#[pymethods]
impl PyCoinWatchlistCache {
    #[new]
    fn new() -> Self {
        Self {
            inner: CoinWatchlistCache::new(),
        }
    }
}

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

fn offers_from_py_list(offers: &Bound<'_, PyList>) -> PyResult<Vec<Value>> {
    offers
        .iter()
        .map(|item| py_any_to_json(&item))
        .collect()
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

#[pyfunction]
#[pyo3(name = "match_watched_coin_ids", signature = (coin_watchlist, observed_coin_ids, /))]
fn match_watched_coin_ids_py(
    coin_watchlist: PyRef<'_, PyCoinWatchlistCache>,
    observed_coin_ids: Vec<String>,
) -> PyResult<Py<PyAny>> {
    let matches = match_watched_coin_ids(&coin_watchlist.inner, &observed_coin_ids);
    Python::attach(|py| {
        let out = PyDict::new(py);
        for (market_id, coin_ids) in matches {
            out.set_item(market_id, coin_ids)?;
        }
        Ok(out.into())
    })
}

#[pyfunction]
#[pyo3(name = "set_watched_coin_ids_for_market", signature = (coin_watchlist, market_id, coin_ids, /))]
fn set_watched_coin_ids_for_market_py(
    coin_watchlist: PyRef<'_, PyCoinWatchlistCache>,
    market_id: String,
    coin_ids: Vec<String>,
) -> PyResult<()> {
    let cache = &coin_watchlist.inner;
    let normalized: HashSet<String> = coin_ids
        .into_iter()
        .map(|coin_id| coin_id.trim().to_ascii_lowercase())
        .filter(|coin_id| !coin_id.is_empty())
        .collect();
    set_watched_coin_ids_for_market(&cache, &market_id, normalized);
    Ok(())
}

#[pyfunction]
#[pyo3(name = "watched_coin_ids_for_market", signature = (coin_watchlist, market_id, /))]
fn watched_coin_ids_for_market_py(
    coin_watchlist: PyRef<'_, PyCoinWatchlistCache>,
    market_id: String,
) -> PyResult<Vec<String>> {
    let mut coin_ids: Vec<String> = watched_coin_ids_for_market(&coin_watchlist.inner, &market_id)
        .into_iter()
        .collect();
    coin_ids.sort();
    Ok(coin_ids)
}

#[pyfunction]
#[pyo3(name = "watchlist_offer_ids_from_store", signature = (db_path, market_id, /))]
fn watchlist_offer_ids_from_store_py(db_path: PathBuf, market_id: String) -> PyResult<Vec<String>> {
    let store = SqliteStore::open(&db_path).map_err(to_py_err)?;
    let mut offer_ids: Vec<String> = watchlist_offer_ids(&store, &market_id)
        .map_err(to_py_err)?
        .into_iter()
        .collect();
    offer_ids.sort();
    Ok(offer_ids)
}

#[pyfunction]
#[pyo3(
    name = "update_market_coin_watchlist_from_offers",
    signature = (db_path, coin_watchlist, market_id, offers, /)
)]
fn update_market_coin_watchlist_from_offers_py(
    db_path: PathBuf,
    coin_watchlist: PyRef<'_, PyCoinWatchlistCache>,
    market_id: String,
    offers: &Bound<'_, PyList>,
) -> PyResult<()> {
    let store = SqliteStore::open(&db_path).map_err(to_py_err)?;
    let cache = &coin_watchlist.inner;
    let offers = offers_from_py_list(offers)?;
    update_market_coin_watchlist_from_offers(&store, &cache, &market_id, &offers).map_err(to_py_err)
}

#[pyfunction]
#[pyo3(name = "build_dexie_size_by_offer_id", signature = (offers, base_asset_id, /))]
fn build_dexie_size_by_offer_id_py(
    offers: &Bound<'_, PyList>,
    base_asset_id: &str,
) -> PyResult<Py<PyAny>> {
    let offers = offers_from_py_list(offers)?;
    let sizes = build_dexie_size_by_offer_id(&offers, base_asset_id);
    Python::attach(|py| {
        let out = PyDict::new(py);
        for (offer_id, size) in sizes {
            out.set_item(offer_id, size)?;
        }
        Ok(out.into())
    })
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("RESEED_MEMPOOL_MAX_AGE_SECONDS", RESEED_MEMPOOL_MAX_AGE_SECONDS)?;
    m.add_function(wrap_pyfunction!(active_offer_counts_by_size, m)?)?;
    m.add_function(wrap_pyfunction!(active_offer_counts_by_size_and_side, m)?)?;
    m.add_function(wrap_pyfunction!(match_watched_coin_ids_py, m)?)?;
    m.add_function(wrap_pyfunction!(set_watched_coin_ids_for_market_py, m)?)?;
    m.add_function(wrap_pyfunction!(watched_coin_ids_for_market_py, m)?)?;
    m.add_function(wrap_pyfunction!(watchlist_offer_ids_from_store_py, m)?)?;
    m.add_function(wrap_pyfunction!(update_market_coin_watchlist_from_offers_py, m)?)?;
    m.add_function(wrap_pyfunction!(build_dexie_size_by_offer_id_py, m)?)?;
    m.add_class::<PyCoinWatchlistCache>()?;
    Ok(())
}
