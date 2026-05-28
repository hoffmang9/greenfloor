use std::collections::BTreeMap;
use std::sync::OnceLock;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};

static PLANNED_ACTION_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static PARALLEL_SKIP_ITEM_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static PARALLEL_QUEUE_ITEM_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static PARALLEL_BATCH_PLAN_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static MANAGED_RETRY_DECISION_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static MANAGED_ACTION_OUTCOME_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static PARALLEL_ACTION_OUTCOME_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static MARKET_BATCH_SELECTION_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static OFFER_STATE_ROW_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static STALE_SWEEP_CANDIDATE_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static STALE_SWEEP_HIT_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static STALE_SWEEP_PROGRESS_CLS: OnceLock<Py<PyAny>> = OnceLock::new();

const ORCHESTRATION_MODULE: &str = "greenfloor.core.cycle_orchestration";

fn cached_class<'py>(
    py: Python<'py>,
    cache: &OnceLock<Py<PyAny>>,
    module: &str,
    name: &str,
) -> PyResult<Bound<'py, PyAny>> {
    if let Some(cls) = cache.get() {
        return Ok(cls.bind(py).clone());
    }
    let cls = PyModule::import(py, module)?.getattr(name)?.unbind();
    let _ = cache.set(cls);
    Ok(cache.get().expect("cached class").bind(py).clone())
}

pub fn planned_action_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &PLANNED_ACTION_CLS, "greenfloor.core.planned_action", "PlannedAction")
}

pub fn parallel_skip_item_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &PARALLEL_SKIP_ITEM_CLS,
        "greenfloor.core.parallel_batch_plan",
        "ParallelSkipItem",
    )
}

pub fn parallel_queue_item_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &PARALLEL_QUEUE_ITEM_CLS,
        "greenfloor.core.parallel_batch_plan",
        "ParallelQueueItem",
    )
}

pub fn parallel_batch_plan_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &PARALLEL_BATCH_PLAN_CLS,
        "greenfloor.core.parallel_batch_plan",
        "ParallelBatchPlan",
    )
}

pub fn managed_retry_decision_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &MANAGED_RETRY_DECISION_CLS,
        "greenfloor.core.managed_retry",
        "ManagedRetryDecision",
    )
}

pub fn managed_action_outcome_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &MANAGED_ACTION_OUTCOME_CLS,
        "greenfloor.core.managed_action_outcome",
        "ManagedActionOutcome",
    )
}

pub fn managed_action_outcome_to_py<'py>(
    py: Python<'py>,
    outcome: &signer_core::ManagedActionOutcome,
) -> PyResult<Bound<'py, PyAny>> {
    let cls = managed_action_outcome_class(py)?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("status", outcome.status.as_str())?;
    kwargs.set_item("reason", &outcome.reason)?;
    kwargs.set_item("offer_id", &outcome.offer_id)?;
    kwargs.set_item("transient_upstream", outcome.transient_upstream)?;
    kwargs.set_item("is_pending_visibility", outcome.is_pending_visibility())?;
    cls.call((), Some(&kwargs))
}

pub fn parallel_action_outcome_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &PARALLEL_ACTION_OUTCOME_CLS, ORCHESTRATION_MODULE, "ParallelActionOutcome")
}

pub fn market_batch_selection_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &MARKET_BATCH_SELECTION_CLS, ORCHESTRATION_MODULE, "MarketBatchSelection")
}

pub fn offer_state_row_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &OFFER_STATE_ROW_CLS, ORCHESTRATION_MODULE, "OfferStateRow")
}

pub fn stale_sweep_candidate_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &STALE_SWEEP_CANDIDATE_CLS, ORCHESTRATION_MODULE, "StaleSweepCandidate")
}

pub fn stale_sweep_hit_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &STALE_SWEEP_HIT_CLS, ORCHESTRATION_MODULE, "StaleSweepHit")
}

pub fn stale_sweep_progress_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &STALE_SWEEP_PROGRESS_CLS, ORCHESTRATION_MODULE, "StaleSweepProgress")
}

pub fn string_i64_map_from_py_dict(dict: &Bound<'_, PyDict>) -> PyResult<BTreeMap<String, i64>> {
    let mut map = BTreeMap::new();
    for (asset_id, amount) in dict.iter() {
        map.insert(asset_id.extract::<String>()?, amount.extract::<i64>()?);
    }
    Ok(map)
}

pub fn to_py_err<E: std::fmt::Display>(err: E) -> PyErr {
    PyValueError::new_err(err.to_string())
}

pub fn dict_from_json_value(py: Python<'_>, value: serde_json::Value) -> PyResult<Py<PyAny>> {
    let json = serde_json::to_string(&value).map_err(to_py_err)?;
    let builtins = py.import("json")?;
    let loads = builtins.getattr("loads")?;
    let obj = loads.call1((json,))?;
    Ok(obj.unbind())
}

pub fn request_dict_to_json(request: &Bound<'_, PyDict>) -> PyResult<serde_json::Value> {
    let py = request.py();
    let json_mod = py.import("json")?;
    let dumps = json_mod.getattr("dumps")?;
    let raw = dumps.call1((request,))?;
    let raw_str: String = raw.extract()?;
    serde_json::from_str(&raw_str).map_err(to_py_err)
}

pub fn dict_to_i64_i64_map(dict: &Bound<'_, PyDict>) -> PyResult<BTreeMap<i64, i64>> {
    let mut map = BTreeMap::new();
    for (key, value) in dict.iter() {
        map.insert(key.extract::<i64>()?, value.extract::<i64>()?);
    }
    Ok(map)
}

pub fn i64_i64_map_to_py_dict<'py>(
    py: Python<'py>,
    map: &BTreeMap<i64, i64>,
) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    for (key, value) in map {
        dict.set_item(*key, *value)?;
    }
    Ok(dict)
}

pub fn string_i64_map_to_py_dict<'py>(
    py: Python<'py>,
    map: &BTreeMap<String, i64>,
) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    for (key, value) in map {
        dict.set_item(key, *value)?;
    }
    Ok(dict)
}

pub fn extract_spendable_profiles(
    profiles: &Bound<'_, PyDict>,
) -> PyResult<BTreeMap<String, signer_core::SpendableAssetProfile>> {
    let mut map = BTreeMap::new();
    for (asset_id, value) in profiles.iter() {
        let profile = value.downcast::<PyDict>().map_err(|_| {
            PyValueError::new_err("spendable profile values must be dicts")
        })?;
        let max_single_known = profile
            .get_item("max_single_known")?
            .ok_or_else(|| {
                PyValueError::new_err("spendable profile max_single_known must be bool")
            })?
            .extract::<bool>()?;
        map.insert(
            asset_id.extract::<String>()?,
            signer_core::SpendableAssetProfile {
                total: profile
                    .get_item("total")?
                    .and_then(|item| item.extract::<i64>().ok())
                    .unwrap_or(0),
                max_single: profile
                    .get_item("max_single")?
                    .and_then(|item| item.extract::<i64>().ok())
                    .unwrap_or(0),
                max_single_known,
            },
        );
    }
    Ok(map)
}
