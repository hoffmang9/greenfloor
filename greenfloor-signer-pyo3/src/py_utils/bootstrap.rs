use std::sync::OnceLock;

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use super::common::cached_class;
use signer_core::{
    plan_bootstrap_mixed_outputs, BootstrapCoin, BootstrapLadderEntry, BootstrapPlan, LadderDeficit,
};

const BOOTSTRAP_MODULE: &str = "greenfloor.offer_bootstrap";

static LADDER_DEFICIT_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static BOOTSTRAP_PLAN_CLS: OnceLock<Py<PyAny>> = OnceLock::new();

fn i64_attr(obj: &Bound<'_, PyAny>, name: &str, default: i64) -> PyResult<i64> {
    if let Ok(dict) = obj.downcast::<PyDict>() {
        return match dict.get_item(name)? {
            None => Ok(default),
            Some(value) => value.extract::<i64>().or(Ok(default)),
        };
    }
    match obj.getattr(name) {
        Ok(value) => value.extract::<i64>().or(Ok(default)),
        Err(_) => Ok(default),
    }
}

fn str_attr_trimmed(obj: &Bound<'_, PyAny>, name: &str, default: &str) -> PyResult<String> {
    if let Ok(dict) = obj.downcast::<PyDict>() {
        return match dict.get_item(name)? {
            None => Ok(default.to_string()),
            Some(value) => Ok(value.extract::<String>().unwrap_or_default().trim().to_string()),
        };
    }
    match obj.getattr(name) {
        Ok(value) => Ok(value.extract::<String>().unwrap_or_default().trim().to_string()),
        Err(_) => Ok(default.to_string()),
    }
}

pub fn bootstrap_ladder_entries_from_py_list(
    list: &Bound<'_, PyList>,
) -> PyResult<Vec<BootstrapLadderEntry>> {
    let mut entries = Vec::with_capacity(list.len());
    for item in list.iter() {
        entries.push(BootstrapLadderEntry {
            size_base_units: i64_attr(&item, "size_base_units", 0)?,
            target_count: i64_attr(&item, "target_count", 0)?,
            split_buffer_count: i64_attr(&item, "split_buffer_count", 0)?,
        });
    }
    Ok(entries)
}

pub fn bootstrap_coins_from_py_list(list: &Bound<'_, PyList>) -> PyResult<Vec<BootstrapCoin>> {
    let mut coins = Vec::with_capacity(list.len());
    for item in list.iter() {
        coins.push(BootstrapCoin {
            id: str_attr_trimmed(&item, "id", "")?,
            amount: i64_attr(&item, "amount", 0)?,
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

pub fn bootstrap_plan_to_py<'py>(
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

pub fn plan_bootstrap_mixed_outputs_from_py<'py>(
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

