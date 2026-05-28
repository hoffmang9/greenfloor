use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use signer_core::{
    build_parallel_reservation_prep, expand_planned_actions, filter_planned_actions_with_positive_repeat,
    plan_parallel_managed_dispatch, plan_parallel_submission_batch, sequential_action_route,
    ParallelActionReservationInput, ParallelBatchPlan, ParallelReservationContext, ParallelReservationPrep,
    SequentialActionRoute,
};

use crate::py_utils::{
    extract_spendable_profiles, managed_retry_decision_class, parallel_batch_plan_class,
    parallel_queue_item_class, parallel_reservation_entry_class, parallel_reservation_prep_class,
    parallel_skip_item_class, parallel_submission_entry_from_py, string_i64_map_from_py_dict,
    string_i64_map_to_py_dict,
};
use crate::strategy_py::{planned_action_from_py, planned_actions_to_py_list};

fn parallel_batch_plan_to_py<'py>(
    py: Python<'py>,
    plan: &ParallelBatchPlan,
) -> PyResult<Bound<'py, PyAny>> {
    let skip_item_cls = parallel_skip_item_class(py)?;
    let queue_item_cls = parallel_queue_item_class(py)?;
    let batch_plan_cls = parallel_batch_plan_class(py)?;

    let skip_items = PyList::empty(py);
    for skip in &plan.skip_items {
        let kwargs = PyDict::new(py);
        kwargs.set_item("submit_index", skip.submit_index)?;
        kwargs.set_item("reason", &skip.reason)?;
        skip_items.append(skip_item_cls.call((), Some(&kwargs))?)?;
    }

    let queue = PyList::empty(py);
    for entry in &plan.queue {
        let kwargs = PyDict::new(py);
        kwargs.set_item("submit_index", entry.submit_index)?;
        kwargs.set_item(
            "requested_amounts",
            string_i64_map_to_py_dict(py, &entry.requested_amounts)?,
        )?;
        kwargs.set_item(
            "available_amounts",
            string_i64_map_to_py_dict(py, &entry.available_amounts)?,
        )?;
        queue.append(queue_item_cls.call((), Some(&kwargs))?)?;
    }

    let kwargs = PyDict::new(py);
    kwargs.set_item("skip_items", skip_items)?;
    kwargs.set_item("queue", queue)?;
    batch_plan_cls.call((), Some(&kwargs))
}

fn parallel_reservation_context_from_py(ctx: &Bound<'_, PyAny>) -> PyResult<ParallelReservationContext> {
    Ok(ParallelReservationContext {
        base_asset_id: ctx.getattr("base_asset_id")?.extract()?,
        quote_asset_id: ctx.getattr("quote_asset_id")?.extract()?,
        fee_asset_id: ctx.getattr("fee_asset_id")?.extract()?,
        fee_amount_mojos: ctx.getattr("fee_amount_mojos")?.extract()?,
        base_unit_mojo_multiplier: ctx.getattr("base_unit_mojo_multiplier")?.extract()?,
        quote_unit_mojo_multiplier: ctx.getattr("quote_unit_mojo_multiplier")?.extract()?,
        quote_price: ctx.getattr("quote_price")?.extract()?,
    })
}

fn parallel_action_reservation_inputs_from_py(
    actions: &Bound<'_, PyList>,
) -> PyResult<Vec<ParallelActionReservationInput>> {
    let mut inputs = Vec::with_capacity(actions.len());
    for item in actions.iter() {
        inputs.push(ParallelActionReservationInput {
            submit_index: item.getattr("submit_index")?.extract()?,
            side: item.getattr("side")?.extract()?,
            size_base_units: item.getattr("size_base_units")?.extract()?,
        });
    }
    Ok(inputs)
}

fn parallel_reservation_prep_to_py<'py>(
    py: Python<'py>,
    prep: &ParallelReservationPrep,
) -> PyResult<Bound<'py, PyAny>> {
    let prep_cls = parallel_reservation_prep_class(py)?;
    let entry_cls = parallel_reservation_entry_class(py)?;
    let entries = PyList::empty(py);
    for entry in &prep.entries {
        let kwargs = PyDict::new(py);
        kwargs.set_item("submit_index", entry.submit_index)?;
        kwargs.set_item(
            "requested_amounts",
            string_i64_map_to_py_dict(py, &entry.requested_amounts)?,
        )?;
        entries.append(entry_cls.call((), Some(&kwargs))?)?;
    }
    let asset_ids = PyList::empty(py);
    for asset_id in &prep.asset_ids {
        asset_ids.append(asset_id)?;
    }
    let kwargs = PyDict::new(py);
    kwargs.set_item("entries", entries)?;
    kwargs.set_item("asset_ids", asset_ids)?;
    prep_cls.call((), Some(&kwargs))
}

pub fn parallel_reservation_prep_from_py(obj: &Bound<'_, PyAny>) -> PyResult<ParallelReservationPrep> {
    let entries_attr = obj.getattr("entries")?;
    let entries_list = entries_attr.downcast::<PyList>()?;
    let mut entries = Vec::with_capacity(entries_list.len());
    for item in entries_list.iter() {
        let requested_attr = item.getattr("requested_amounts")?;
        let requested = requested_attr
            .downcast::<PyDict>()
            .map_err(|_| PyValueError::new_err("requested_amounts must be a dict"))?;
        entries.push(signer_core::ParallelReservationEntry {
            submit_index: item.getattr("submit_index")?.extract()?,
            requested_amounts: string_i64_map_from_py_dict(requested)?,
        });
    }
    let asset_ids_attr = obj.getattr("asset_ids")?;
    let asset_ids_list = asset_ids_attr.downcast::<PyList>()?;
    let mut asset_ids = Vec::with_capacity(asset_ids_list.len());
    for item in asset_ids_list.iter() {
        asset_ids.push(item.extract::<String>()?);
    }
    Ok(ParallelReservationPrep { entries, asset_ids })
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
        rust_entries.push(parallel_submission_entry_from_py(&item)?);
    }
    let plan = plan_parallel_submission_batch(&rust_entries);
    Python::attach(|py| Ok(parallel_batch_plan_to_py(py, &plan)?.into()))
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

#[pyfunction]
#[pyo3(name = "build_parallel_reservation_prep")]
fn build_parallel_reservation_prep_py(
    actions: &Bound<'_, PyList>,
    ctx: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let rust_actions = parallel_action_reservation_inputs_from_py(actions)?;
    let rust_ctx = parallel_reservation_context_from_py(ctx)?;
    let prep = build_parallel_reservation_prep(&rust_actions, &rust_ctx);
    Python::attach(|py| Ok(parallel_reservation_prep_to_py(py, &prep)?.into()))
}

#[pyfunction]
#[pyo3(name = "plan_parallel_managed_dispatch")]
fn plan_parallel_managed_dispatch_py(
    prep: &Bound<'_, PyAny>,
    spendable_profiles: &Bound<'_, PyDict>,
) -> PyResult<Py<PyAny>> {
    let rust_prep = parallel_reservation_prep_from_py(prep)?;
    let profiles = extract_spendable_profiles(spendable_profiles)?;
    let plan = plan_parallel_managed_dispatch(&rust_prep, &profiles);
    Python::attach(|py| Ok(parallel_batch_plan_to_py(py, &plan)?.into()))
}

pub fn register_execution(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(sequential_action_route_py, m)?)?;
    m.add_function(wrap_pyfunction!(plan_parallel_submission_batch_py, m)?)?;
    m.add_function(wrap_pyfunction!(expand_planned_actions_py, m)?)?;
    m.add_function(wrap_pyfunction!(filter_planned_actions_with_positive_repeat_py, m)?)?;
    m.add_function(wrap_pyfunction!(build_parallel_reservation_prep_py, m)?)?;
    m.add_function(wrap_pyfunction!(plan_parallel_managed_dispatch_py, m)?)?;
    Ok(())
}
