use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use engine_core::{
    expand_planned_actions, filter_planned_actions_with_positive_repeat,
    plan_parallel_managed_dispatch, sequential_action_route, ParallelBatchPlan,
    ParallelReservationContext, SequentialActionRoute,
};

use crate::py_utils::{
    extract_spendable_profiles, parallel_batch_plan_class, parallel_queue_item_class,
    parallel_skip_item_class, string_i64_map_to_py_dict,
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

fn parallel_reservation_context_from_py(
    ctx: &Bound<'_, PyAny>,
) -> PyResult<ParallelReservationContext> {
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
fn filter_planned_actions_with_positive_repeat_py(
    actions: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let list = actions.downcast::<PyList>()?;
    let mut rust_actions = Vec::with_capacity(list.len());
    for item in list.iter() {
        rust_actions.push(planned_action_from_py(&item)?);
    }
    let filtered = filter_planned_actions_with_positive_repeat(&rust_actions);
    Python::attach(|py| planned_actions_to_py_list(py, &filtered))
}

#[pyfunction]
#[pyo3(name = "plan_parallel_managed_dispatch")]
fn plan_parallel_managed_dispatch_py(
    actions: &Bound<'_, PyList>,
    ctx: &Bound<'_, PyAny>,
    spendable_profiles: &Bound<'_, PyDict>,
) -> PyResult<Py<PyAny>> {
    let mut rust_actions = Vec::with_capacity(actions.len());
    for item in actions.iter() {
        rust_actions.push(planned_action_from_py(&item)?);
    }
    let rust_ctx = parallel_reservation_context_from_py(ctx)?;
    let profiles = extract_spendable_profiles(spendable_profiles)?;
    let plan = plan_parallel_managed_dispatch(&rust_actions, &rust_ctx, &profiles);
    Python::attach(|py| Ok(parallel_batch_plan_to_py(py, &plan)?.into()))
}

pub fn register_execution(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(sequential_action_route_py, m)?)?;
    m.add_function(wrap_pyfunction!(expand_planned_actions_py, m)?)?;
    m.add_function(wrap_pyfunction!(
        filter_planned_actions_with_positive_repeat_py,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(plan_parallel_managed_dispatch_py, m)?)?;
    Ok(())
}
