use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyModule};

static PLANNED_ACTION_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static PARALLEL_SKIP_ITEM_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static PARALLEL_QUEUE_ITEM_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static PARALLEL_BATCH_PLAN_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static MANAGED_RETRY_DECISION_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static MANAGED_ACTION_OUTCOME_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static MARKET_BATCH_SELECTION_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static OFFER_STATE_ROW_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static STALE_SWEEP_CANDIDATE_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static STALE_SWEEP_HIT_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static STALE_SWEEP_PROGRESS_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static RESEED_GAP_PLAN_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static RESEED_SKIP_REASON_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static CYCLE_OFFER_TRANSITION_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static BUCKET_SPEC_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static COIN_OP_PLAN_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static SPLIT_COIN_PLAN_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static SPLIT_COMBINE_PREREQ_PLAN_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static SPLIT_SKIP_PLAN_CLS: OnceLock<Py<PyAny>> = OnceLock::new();

const ORCHESTRATION_MODULE: &str = "greenfloor.core.cycle_orchestration";
const CYCLE_RESEED_MODULE: &str = "greenfloor.core.cycle_reseed";
const OFFER_RECONCILE_MODULE: &str = "greenfloor.core.offer_reconcile";
const COIN_OPS_MODULE: &str = "greenfloor.core.coin_ops";

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

pub fn reseed_gap_plan_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &RESEED_GAP_PLAN_CLS, CYCLE_RESEED_MODULE, "ReseedGapPlan")
}

pub fn reseed_skip_reason_to_py<'py>(
    py: Python<'py>,
    reason: signer_core::ReseedSkipReason,
) -> PyResult<Bound<'py, PyAny>> {
    let cls = cached_class(py, &RESEED_SKIP_REASON_CLS, CYCLE_RESEED_MODULE, "ReseedSkipReason")?;
    cls.call1((reason.label(),))
}

pub fn reseed_gap_plan_to_py<'py>(
    py: Python<'py>,
    plan: &signer_core::ReseedGapPlan,
) -> PyResult<Bound<'py, PyAny>> {
    let cls = reseed_gap_plan_class(py)?;
    let kwargs = PyDict::new(py);
    kwargs.set_item(
        "actions",
        crate::strategy_py::planned_actions_to_py_list(py, &plan.actions)?,
    )?;
    match plan.skip_reason {
        Some(reason) => kwargs.set_item("skip_reason", reseed_skip_reason_to_py(py, reason)?)?,
        None => kwargs.set_item("skip_reason", py.None())?,
    }
    kwargs.set_item("missing_by_size", i64_i64_map_to_py_dict(py, &plan.missing_by_size)?)?;
    cls.call((), Some(&kwargs))
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

pub fn cycle_offer_transition_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &CYCLE_OFFER_TRANSITION_CLS,
        OFFER_RECONCILE_MODULE,
        "CycleOfferTransition",
    )
}

pub fn bucket_spec_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &BUCKET_SPEC_CLS, COIN_OPS_MODULE, "BucketSpec")
}

pub fn coin_op_plan_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &COIN_OP_PLAN_CLS, COIN_OPS_MODULE, "CoinOpPlan")
}

fn coin_op_kind_from_py(obj: &Bound<'_, PyAny>) -> PyResult<signer_core::CoinOpKind> {
    let op_type: String = obj.getattr("op_type")?.extract()?;
    match op_type.as_str() {
        "split" => Ok(signer_core::CoinOpKind::Split),
        "combine" => Ok(signer_core::CoinOpKind::Combine),
        other => Err(PyValueError::new_err(format!("invalid coin op type: {other}"))),
    }
}

pub fn bucket_spec_from_py(obj: &Bound<'_, PyAny>) -> PyResult<signer_core::BucketSpec> {
    let cls = bucket_spec_class(obj.py())?;
    if !obj.is_instance(&cls)? {
        return Err(PyTypeError::new_err("expected BucketSpec"));
    }
    Ok(signer_core::BucketSpec {
        size_base_units: obj.getattr("size_base_units")?.extract()?,
        target_count: obj.getattr("target_count")?.extract()?,
        split_buffer_count: obj.getattr("split_buffer_count")?.extract()?,
        combine_when_excess_factor: obj.getattr("combine_when_excess_factor")?.extract()?,
        current_count: obj.getattr("current_count")?.extract()?,
    })
}

pub fn coin_op_plan_from_py(obj: &Bound<'_, PyAny>) -> PyResult<signer_core::CoinOpPlan> {
    let cls = coin_op_plan_class(obj.py())?;
    if !obj.is_instance(&cls)? {
        return Err(PyTypeError::new_err("expected CoinOpPlan"));
    }
    Ok(signer_core::CoinOpPlan {
        op_type: coin_op_kind_from_py(obj)?,
        size_base_units: obj.getattr("size_base_units")?.extract()?,
        op_count: obj.getattr("op_count")?.extract()?,
        reason: obj.getattr("reason")?.extract()?,
    })
}

pub fn coin_op_plan_to_py<'py>(
    py: Python<'py>,
    plan: &signer_core::CoinOpPlan,
) -> PyResult<Bound<'py, PyAny>> {
    let cls = coin_op_plan_class(py)?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("op_type", plan.op_type.as_str())?;
    kwargs.set_item("size_base_units", plan.size_base_units)?;
    kwargs.set_item("op_count", plan.op_count)?;
    kwargs.set_item("reason", &plan.reason)?;
    cls.call((), Some(&kwargs))
}

pub fn coin_op_plans_from_py_list(plans: &Bound<'_, PyList>) -> PyResult<Vec<signer_core::CoinOpPlan>> {
    let mut parsed = Vec::with_capacity(plans.len());
    for item in plans.iter() {
        parsed.push(coin_op_plan_from_py(&item)?);
    }
    Ok(parsed)
}

pub fn split_coin_plan_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &SPLIT_COIN_PLAN_CLS, COIN_OPS_MODULE, "SplitCoinPlan")
}

pub fn split_combine_prereq_plan_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &SPLIT_COMBINE_PREREQ_PLAN_CLS,
        COIN_OPS_MODULE,
        "SplitCombinePrereqPlan",
    )
}

pub fn split_skip_plan_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &SPLIT_SKIP_PLAN_CLS, COIN_OPS_MODULE, "SplitSkipPlan")
}

pub fn spendable_coins_from_py_list(list: &Bound<'_, PyList>) -> PyResult<Vec<signer_core::SpendableCoin>> {
    let mut coins = Vec::with_capacity(list.len());
    for item in list.iter() {
        let dict = item.downcast::<PyDict>().map_err(|_| {
            PyTypeError::new_err("spendable coins must be dict rows")
        })?;
        let id = dict
            .get_item("id")?
            .map(|value| value.extract::<String>().unwrap_or_default())
            .unwrap_or_default()
            .trim()
            .to_string();
        let amount = match dict.get_item("amount")? {
            None => 0,
            Some(value) => value.extract::<i64>().unwrap_or(0),
        };
        coins.push(signer_core::SpendableCoin { id, amount });
    }
    Ok(coins)
}

pub fn exclude_coin_ids_from_py_optional(
    exclude: Option<&Bound<'_, PyAny>>,
) -> PyResult<HashSet<String>> {
    let Some(value) = exclude else {
        return Ok(HashSet::new());
    };
    if value.is_none() {
        return Ok(HashSet::new());
    }
    let mut set = HashSet::new();
    for item in value.try_iter()? {
        set.insert(item?.extract::<String>()?);
    }
    Ok(set)
}

pub fn split_auto_select_plan_to_py<'py>(
    py: Python<'py>,
    plan: signer_core::SplitAutoSelectPlan,
) -> PyResult<Bound<'py, PyAny>> {
    match plan {
        signer_core::SplitAutoSelectPlan::Coin(coin) => {
            let cls = split_coin_plan_class(py)?;
            let kwargs = PyDict::new(py);
            kwargs.set_item("coin_id", &coin.coin_id)?;
            kwargs.set_item("selected_amount_mojos", coin.selected_amount_mojos)?;
            cls.call((), Some(&kwargs))
        }
        signer_core::SplitAutoSelectPlan::CombinePrereq(prereq) => {
            let cls = split_combine_prereq_plan_class(py)?;
            let kwargs = PyDict::new(py);
            let ids = PyList::new(py, &prereq.input_coin_ids)?;
            kwargs.set_item("input_coin_ids", ids)?;
            kwargs.set_item("target_amount", prereq.target_amount)?;
            kwargs.set_item("selected_total", prereq.selected_total)?;
            kwargs.set_item("exact_match", prereq.exact_match)?;
            kwargs.set_item("cap_applied", prereq.cap_applied)?;
            kwargs.set_item(
                "selected_count_before_cap",
                prereq.selected_count_before_cap as i64,
            )?;
            kwargs.set_item("combine_input_cap", prereq.combine_input_cap)?;
            cls.call((), Some(&kwargs))
        }
        signer_core::SplitAutoSelectPlan::Skip(skip) => {
            let cls = split_skip_plan_class(py)?;
            let kwargs = PyDict::new(py);
            kwargs.set_item("reason", &skip.reason)?;
            match skip.data {
                Some(data) => {
                    let data_dict = PyDict::new(py);
                    data_dict.set_item("selected_coin_id", &data.selected_coin_id)?;
                    data_dict.set_item("selected_amount_mojos", data.selected_amount_mojos)?;
                    data_dict.set_item("required_amount_mojos", data.required_amount_mojos)?;
                    data_dict.set_item("remainder_mojos", data.remainder_mojos)?;
                    data_dict.set_item("minimum_allowed_mojos", data.minimum_allowed_mojos)?;
                    kwargs.set_item("data", data_dict)?;
                }
                None => kwargs.set_item("data", py.None())?,
            }
            cls.call((), Some(&kwargs))
        }
    }
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
