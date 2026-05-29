use std::sync::OnceLock;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use super::common::cached_class;
use signer_core::{
    plan_bootstrap_mixed_outputs, BootstrapCoin, BootstrapLadderEntry, BootstrapPlan, LadderDeficit,
};

const BOOTSTRAP_MODULE: &str = "greenfloor.offer_bootstrap";

static LADDER_DEFICIT_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static BOOTSTRAP_PLAN_CLS: OnceLock<Py<PyAny>> = OnceLock::new();

fn required_i64_attr(obj: &Bound<'_, PyAny>, name: &str, label: &str) -> PyResult<i64> {
    if let Ok(dict) = obj.downcast::<PyDict>() {
        let value = dict
            .get_item(name)?
            .ok_or_else(|| PyValueError::new_err(format!("{label} missing required field: {name}")))?;
        return value
            .extract::<i64>()
            .map_err(|_| PyValueError::new_err(format!("{label}.{name} must be an integer")));
    }
    let value = obj.getattr(name).map_err(|_| {
        PyValueError::new_err(format!("{label} missing required attribute: {name}"))
    })?;
    value
        .extract::<i64>()
        .map_err(|_| PyValueError::new_err(format!("{label}.{name} must be an integer")))
}

fn required_coin_amount(obj: &Bound<'_, PyAny>, index: usize) -> PyResult<i64> {
    let label = format!("spendable_coins[{index}]");
    if let Ok(dict) = obj.downcast::<PyDict>() {
        let value = dict.get_item("amount")?.ok_or_else(|| {
            PyValueError::new_err(format!("{label} missing required field: amount"))
        })?;
        return value
            .extract::<i64>()
            .map_err(|_| PyValueError::new_err(format!("{label}.amount must be an integer")));
    }
    let value = obj.getattr("amount").map_err(|_| {
        PyValueError::new_err(format!("{label} missing required attribute: amount"))
    })?;
    value
        .extract::<i64>()
        .map_err(|_| PyValueError::new_err(format!("{label}.amount must be an integer")))
}

fn optional_coin_id(obj: &Bound<'_, PyAny>) -> PyResult<String> {
    if let Ok(dict) = obj.downcast::<PyDict>() {
        return match dict.get_item("id")? {
            None => Ok(String::new()),
            Some(value) => Ok(value.extract::<String>().unwrap_or_default().trim().to_string()),
        };
    }
    match obj.getattr("id") {
        Ok(value) => Ok(value.extract::<String>().unwrap_or_default().trim().to_string()),
        Err(_) => Ok(String::new()),
    }
}

fn bootstrap_ladder_entries_from_py_list(
    list: &Bound<'_, PyList>,
) -> PyResult<Vec<BootstrapLadderEntry>> {
    let mut entries = Vec::with_capacity(list.len());
    for (index, item) in list.iter().enumerate() {
        let label = format!("sell_ladder[{index}]");
        entries.push(BootstrapLadderEntry {
            size_base_units: required_i64_attr(&item, "size_base_units", &label)?,
            target_count: required_i64_attr(&item, "target_count", &label)?,
            split_buffer_count: required_i64_attr(&item, "split_buffer_count", &label)?,
        });
    }
    Ok(entries)
}

fn bootstrap_coins_from_py_list(list: &Bound<'_, PyList>) -> PyResult<Vec<BootstrapCoin>> {
    let mut coins = Vec::with_capacity(list.len());
    for (index, item) in list.iter().enumerate() {
        coins.push(BootstrapCoin {
            id: optional_coin_id(&item)?,
            amount: required_coin_amount(&item, index)?,
        });
    }
    Ok(coins)
}

fn ladder_deficit_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &LADDER_DEFICIT_CLS, BOOTSTRAP_MODULE, "LadderDeficit")
}

fn bootstrap_plan_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &BOOTSTRAP_PLAN_CLS, BOOTSTRAP_MODULE, "BootstrapPlan")
}

fn ladder_deficit_to_py<'py>(py: Python<'py>, deficit: &LadderDeficit) -> PyResult<Bound<'py, PyAny>> {
    let cls = ladder_deficit_class(py)?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("size_base_units", deficit.size_base_units)?;
    kwargs.set_item("required_count", deficit.required_count)?;
    kwargs.set_item("current_count", deficit.current_count)?;
    kwargs.set_item("deficit_count", deficit.deficit_count)?;
    cls.call((), Some(&kwargs))
}

fn bootstrap_plan_to_py<'py>(
    py: Python<'py>,
    plan: &BootstrapPlan,
) -> PyResult<Bound<'py, PyAny>> {
    let cls = bootstrap_plan_class(py)?;
    let deficits = PyList::empty(py);
    for deficit in &plan.deficits {
        deficits.append(ladder_deficit_to_py(py, deficit)?)?;
    }
    let output_amounts = PyList::empty(py);
    for amount in &plan.output_amounts_base_units {
        output_amounts.append(*amount)?;
    }
    let kwargs = PyDict::new(py);
    kwargs.set_item("source_coin_id", &plan.source_coin_id)?;
    kwargs.set_item("source_amount", plan.source_amount)?;
    kwargs.set_item("output_amounts_base_units", output_amounts)?;
    kwargs.set_item("total_output_amount", plan.total_output_amount)?;
    kwargs.set_item("change_amount", plan.change_amount)?;
    kwargs.set_item("deficits", deficits)?;
    cls.call((), Some(&kwargs))
}

pub(crate) fn plan_bootstrap_mixed_outputs_from_py<'py>(
    py: Python<'py>,
    sell_ladder: &Bound<'py, PyList>,
    spendable_coins: &Bound<'py, PyList>,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    let ladder = bootstrap_ladder_entries_from_py_list(sell_ladder)?;
    let coins = bootstrap_coins_from_py_list(spendable_coins)?;
    let plan = plan_bootstrap_mixed_outputs(&ladder, &coins);
    match plan {
        Some(plan) => Ok(Some(bootstrap_plan_to_py(py, &plan)?)),
        None => Ok(None),
    }
}
