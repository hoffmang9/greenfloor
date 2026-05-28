use std::collections::HashSet;
use std::sync::OnceLock;

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use super::common::cached_class;
use signer_core::{CombineInputSelectionMode, SplitAutoSelectPlan, SplitPlanningProfile};

const COIN_OPS_MODULE: &str = "greenfloor.core.coin_ops";

static BUCKET_SPEC_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static COIN_OP_PLAN_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static SPLIT_COIN_PLAN_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static SPLIT_COMBINE_PREREQ_PLAN_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static SPLIT_SKIP_PLAN_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static SPLIT_DENOMINATION_READINESS_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static COMBINE_DENOMINATION_READINESS_CLS: OnceLock<Py<PyAny>> = OnceLock::new();

fn enum_label_from_py(obj: &Bound<'_, PyAny>) -> PyResult<String> {
    if let Ok(label) = obj.extract::<String>() {
        return Ok(label);
    }
    obj.getattr("value")?.extract::<String>()
}

pub fn split_planning_profile_from_py(obj: &Bound<'_, PyAny>) -> PyResult<SplitPlanningProfile> {
    let label = enum_label_from_py(obj)?;
    SplitPlanningProfile::from_label(&label)
        .ok_or_else(|| PyValueError::new_err(format!("invalid split planning profile: {label}")))
}

pub fn combine_input_selection_mode_from_py(
    obj: &Bound<'_, PyAny>,
) -> PyResult<CombineInputSelectionMode> {
    let label = enum_label_from_py(obj)?;
    CombineInputSelectionMode::from_label(&label)
        .ok_or_else(|| PyValueError::new_err(format!("invalid combine selection mode: {label}")))
}

pub fn bucket_spec_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &BUCKET_SPEC_CLS, COIN_OPS_MODULE, "BucketSpec")
}

pub fn coin_op_plan_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &COIN_OP_PLAN_CLS, COIN_OPS_MODULE, "CoinOpPlan")
}

pub fn split_denomination_readiness_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &SPLIT_DENOMINATION_READINESS_CLS,
        COIN_OPS_MODULE,
        "SplitDenominationReadiness",
    )
}

pub fn split_denomination_readiness_to_py<'py>(
    py: Python<'py>,
    gate: &signer_core::CoinSplitGateResult,
) -> PyResult<Bound<'py, PyAny>> {
    let cls = split_denomination_readiness_class(py)?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("asset_id", &gate.asset_id)?;
    kwargs.set_item("size_base_units", gate.size_base_units)?;
    kwargs.set_item("required_min_count", gate.required_min_count)?;
    kwargs.set_item("current_count", gate.current_count)?;
    kwargs.set_item("larger_reserve_coin_count", gate.larger_reserve_coin_count)?;
    kwargs.set_item("extra_denom_coin_count", gate.extra_denom_coin_count)?;
    kwargs.set_item("reserve_ready", gate.reserve_ready)?;
    kwargs.set_item("ready", gate.ready)?;
    cls.call((), Some(&kwargs))
}

pub fn combine_denomination_readiness_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &COMBINE_DENOMINATION_READINESS_CLS,
        COIN_OPS_MODULE,
        "CombineDenominationReadiness",
    )
}

pub fn combine_denomination_readiness_to_py<'py>(
    py: Python<'py>,
    gate: &signer_core::CoinCombineGateResult,
) -> PyResult<Bound<'py, PyAny>> {
    let cls = combine_denomination_readiness_class(py)?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("asset_id", &gate.asset_id)?;
    kwargs.set_item("size_base_units", gate.size_base_units)?;
    kwargs.set_item("max_allowed_count", gate.max_allowed_count)?;
    kwargs.set_item("current_count", gate.current_count)?;
    kwargs.set_item("ready", gate.ready)?;
    cls.call((), Some(&kwargs))
}

fn coin_op_kind_from_py(obj: &Bound<'_, PyAny>) -> PyResult<signer_core::CoinOpKind> {
    let op_type: String = obj.getattr("op_type")?.extract()?;
    match op_type.as_str() {
        "split" => Ok(signer_core::CoinOpKind::Split),
        "combine" => Ok(signer_core::CoinOpKind::Combine),
        other => Err(PyValueError::new_err(format!(
            "invalid coin op type: {other}"
        ))),
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

pub fn coin_op_plans_from_py_list(
    plans: &Bound<'_, PyList>,
) -> PyResult<Vec<signer_core::CoinOpPlan>> {
    let mut parsed = Vec::with_capacity(plans.len());
    for item in plans.iter() {
        parsed.push(coin_op_plan_from_py(&item)?);
    }
    Ok(parsed)
}

fn split_coin_plan_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &SPLIT_COIN_PLAN_CLS, COIN_OPS_MODULE, "SplitCoinPlan")
}

fn split_combine_prereq_plan_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &SPLIT_COMBINE_PREREQ_PLAN_CLS,
        COIN_OPS_MODULE,
        "SplitCombinePrereqPlan",
    )
}

fn split_skip_plan_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &SPLIT_SKIP_PLAN_CLS, COIN_OPS_MODULE, "SplitSkipPlan")
}

pub fn spendable_coins_from_py_list(
    list: &Bound<'_, PyList>,
) -> PyResult<Vec<signer_core::SpendableCoin>> {
    let mut coins = Vec::with_capacity(list.len());
    for item in list.iter() {
        let dict = item
            .downcast::<PyDict>()
            .map_err(|_| PyTypeError::new_err("spendable coins must be dict rows"))?;
        let id = dict
            .get_item("id")?
            .map(|value| value.extract::<String>().unwrap_or_default())
            .unwrap_or_default()
            .trim()
            .to_string();
        if id.is_empty() {
            continue;
        }
        let amount = match dict.get_item("amount")? {
            None => 0,
            Some(value) => match value.extract::<i64>() {
                Ok(amount) => amount,
                Err(_) => continue,
            },
        };
        if amount <= 0 {
            continue;
        }
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
    plan: SplitAutoSelectPlan,
) -> PyResult<Bound<'py, PyAny>> {
    match plan {
        SplitAutoSelectPlan::Coin(coin) => {
            let cls = split_coin_plan_class(py)?;
            let kwargs = PyDict::new(py);
            kwargs.set_item("coin_id", &coin.coin_id)?;
            kwargs.set_item("selected_amount_mojos", coin.selected_amount_mojos)?;
            cls.call((), Some(&kwargs))
        }
        SplitAutoSelectPlan::CombinePrereq(prereq) => {
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
        SplitAutoSelectPlan::Skip(skip) => {
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
