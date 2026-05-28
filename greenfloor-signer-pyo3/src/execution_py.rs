use std::collections::BTreeMap;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use signer_core::{
    expand_planned_actions, filter_planned_actions_with_positive_repeat,
    plan_parallel_submission_batch, sequential_action_route, ParallelBatchPlan,
    ParallelSubmissionEntry, SequentialActionRoute,
};

use crate::py_utils::{extract_spendable_profiles, i64_i64_map_to_py_dict, string_i64_map_to_py_dict};
use crate::strategy_py::{planned_action_from_py, planned_actions_to_py_list};

fn parallel_batch_plan_to_py_dict<'py>(
    py: Python<'py>,
    plan: &ParallelBatchPlan,
) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    let skip_items = PyList::empty(py);
    for skip in &plan.skip_items {
        let item = PyDict::new(py);
        item.set_item("submit_index", skip.submit_index)?;
        item.set_item("reason", &skip.reason)?;
        skip_items.append(item)?;
    }
    let queue = PyList::empty(py);
    for entry in &plan.queue {
        let item = PyDict::new(py);
        item.set_item("submit_index", entry.submit_index)?;
        item.set_item(
            "requested_amounts",
            string_i64_map_to_py_dict(py, &entry.requested_amounts)?,
        )?;
        item.set_item(
            "available_amounts",
            string_i64_map_to_py_dict(py, &entry.available_amounts)?,
        )?;
        queue.append(item)?;
    }
    result.set_item("skip_items", skip_items)?;
    result.set_item("queue", queue)?;
    Ok(result)
}

#[pyfunction]
#[pyo3(name = "sequential_action_route")]
fn sequential_action_route_py(
    runtime_dry_run: bool,
    program_present: bool,
    managed_backend_available: bool,
) -> &'static str {
    match sequential_action_route(runtime_dry_run, program_present, managed_backend_available) {
        SequentialActionRoute::DryRunPlanned => "dry_run_planned",
        SequentialActionRoute::Managed => "managed",
        SequentialActionRoute::Local => "local",
        SequentialActionRoute::SkipNoProgram => "skip_no_program",
        SequentialActionRoute::SkipNoManagedBackend => "skip_no_managed_backend",
    }
}

#[pyfunction]
#[pyo3(name = "plan_parallel_submission_batch")]
fn plan_parallel_submission_batch_py(entries: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    let list = entries.downcast::<PyList>()?;
    let mut rust_entries = Vec::with_capacity(list.len());
    for item in list.iter() {
        let row = item.downcast::<PyDict>()?;
        let submit_index = row
            .get_item("submit_index")?
            .ok_or_else(|| PyValueError::new_err("submit_index required"))?
            .extract::<usize>()?;
        let requested_any = row
            .get_item("requested_amounts")?
            .ok_or_else(|| PyValueError::new_err("requested_amounts required"))?;
        let requested = requested_any.downcast::<PyDict>()?;
        let profiles_any = row
            .get_item("spendable_profiles")?
            .ok_or_else(|| PyValueError::new_err("spendable_profiles required"))?;
        let profiles = profiles_any.downcast::<PyDict>()?;
        let mut requested_amounts = BTreeMap::new();
        for (asset_id, amount) in requested.iter() {
            requested_amounts.insert(asset_id.extract::<String>()?, amount.extract::<i64>()?);
        }
        rust_entries.push(ParallelSubmissionEntry {
            submit_index,
            requested_amounts,
            spendable_profiles: extract_spendable_profiles(profiles)?,
        });
    }
    let plan = plan_parallel_submission_batch(&rust_entries);
    Python::attach(|py| Ok(parallel_batch_plan_to_py_dict(py, &plan)?.into()))
}

#[pyfunction]
#[pyo3(name = "expand_planned_actions")]
fn expand_planned_actions_py(actions: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    let list = actions.downcast::<PyList>()?;
    let mut rust_actions = Vec::with_capacity(list.len());
    for item in list.iter() {
        rust_actions.push(planned_action_from_py(&item)?);
    }
    let expanded = expand_planned_actions(&rust_actions);
    Python::attach(|py| planned_actions_to_py_list(py, &expanded))
}

#[pyfunction]
#[pyo3(name = "filter_planned_actions_with_positive_repeat")]
fn filter_planned_actions_with_positive_repeat_py(actions: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    let list = actions.downcast::<PyList>()?;
    let mut rust_actions = Vec::with_capacity(list.len());
    for item in list.iter() {
        rust_actions.push(planned_action_from_py(&item)?);
    }
    let filtered = filter_planned_actions_with_positive_repeat(&rust_actions);
    Python::attach(|py| planned_actions_to_py_list(py, &filtered))
}

pub fn register_execution(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(sequential_action_route_py, m)?)?;
    m.add_function(wrap_pyfunction!(plan_parallel_submission_batch_py, m)?)?;
    m.add_function(wrap_pyfunction!(expand_planned_actions_py, m)?)?;
    m.add_function(wrap_pyfunction!(filter_planned_actions_with_positive_repeat_py, m)?)?;
    Ok(())
}
