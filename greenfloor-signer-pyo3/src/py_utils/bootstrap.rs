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

fn field_value<'a>(
    obj: &'a Bound<'_, PyAny>,
    name: &str,
    label: &str,
) -> PyResult<Bound<'a, PyAny>> {
    if let Ok(dict) = obj.downcast::<PyDict>() {
        return dict.get_item(name)?.ok_or_else(|| {
            PyValueError::new_err(format!("{label} missing required field: {name}"))
        });
    }
    obj.getattr(name).map_err(|_| {
        PyValueError::new_err(format!("{label} missing required attribute: {name}"))
    })
}

fn extract_i64(obj: &Bound<'_, PyAny>, name: &str, label: &str) -> PyResult<i64> {
    field_value(obj, name, label)?.extract::<i64>().map_err(|_| {
        PyValueError::new_err(format!("{label}.{name} must be an integer"))
    })
}

fn extract_optional_str(obj: &Bound<'_, PyAny>, name: &str, label: &str) -> PyResult<String> {
    let value = if let Ok(dict) = obj.downcast::<PyDict>() {
        dict.get_item(name)?
    } else {
        obj.getattr(name).ok()
    };
    match value {
        None => Ok(String::new()),
        Some(raw) => raw
            .extract::<String>()
            .map_err(|_| PyValueError::new_err(format!("{label}.{name} must be a string")))
            .map(|text| text.trim().to_string()),
    }
}

fn bootstrap_ladder_entries_from_py_list(
    list: &Bound<'_, PyList>,
) -> PyResult<Vec<BootstrapLadderEntry>> {
    let mut entries = Vec::with_capacity(list.len());
    for (index, item) in list.iter().enumerate() {
        let label = format!("sell_ladder[{index}]");
        entries.push(BootstrapLadderEntry {
            size_base_units: extract_i64(&item, "size_base_units", &label)?,
            target_count: extract_i64(&item, "target_count", &label)?,
            split_buffer_count: extract_i64(&item, "split_buffer_count", &label)?,
        });
    }
    Ok(entries)
}

fn bootstrap_coins_from_py_list(list: &Bound<'_, PyList>) -> PyResult<Vec<BootstrapCoin>> {
    let mut coins = Vec::with_capacity(list.len());
    for (index, item) in list.iter().enumerate() {
        let label = format!("spendable_coins[{index}]");
        coins.push(BootstrapCoin {
            id: extract_optional_str(&item, "id", &label)?,
            amount: extract_i64(&item, "amount", &label)?,
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
