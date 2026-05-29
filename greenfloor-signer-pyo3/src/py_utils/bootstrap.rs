use std::sync::OnceLock;

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use super::common::cached_class;
use signer_core::{
    bootstrap_early_phase_from_kind, bootstrap_executed_phase_from_kind,
    plan_bootstrap_mixed_outputs, BootstrapCoin, BootstrapPlan, BootstrapPlanOutcome,
    BootstrapPhaseSnapshot, LadderDeficit, PlannerLadderRow,
};

const BOOTSTRAP_MODULE: &str = "greenfloor.offer_bootstrap";

static PLANNER_LADDER_ROW_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static BOOTSTRAP_COIN_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static LADDER_DEFICIT_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static BOOTSTRAP_PLAN_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static BOOTSTRAP_PLAN_OUTCOME_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static BOOTSTRAP_PHASE_RESULT_CLS: OnceLock<Py<PyAny>> = OnceLock::new();

fn planner_ladder_row_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &PLANNER_LADDER_ROW_CLS,
        BOOTSTRAP_MODULE,
        "PlannerLadderRow",
    )
}

fn bootstrap_coin_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &BOOTSTRAP_COIN_CLS, BOOTSTRAP_MODULE, "BootstrapCoin")
}

fn ladder_deficit_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &LADDER_DEFICIT_CLS, BOOTSTRAP_MODULE, "LadderDeficit")
}

fn bootstrap_plan_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(py, &BOOTSTRAP_PLAN_CLS, BOOTSTRAP_MODULE, "BootstrapPlan")
}

fn bootstrap_phase_result_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &BOOTSTRAP_PHASE_RESULT_CLS,
        BOOTSTRAP_MODULE,
        "BootstrapPhaseResult",
    )
}

fn bootstrap_plan_outcome_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &BOOTSTRAP_PLAN_OUTCOME_CLS,
        BOOTSTRAP_MODULE,
        "BootstrapPlanOutcome",
    )
}

fn require_instance<'a, 'py>(
    item: &'a Bound<'py, PyAny>,
    cls: &Bound<'py, PyAny>,
    label: &str,
    expected: &str,
) -> PyResult<&'a Bound<'py, PyAny>> {
    if !item.is_instance(cls)? {
        return Err(PyTypeError::new_err(format!(
            "{label} must be a {expected} instance"
        )));
    }
    Ok(item)
}

fn extract_i64(obj: &Bound<'_, PyAny>, name: &str, label: &str) -> PyResult<i64> {
    obj.getattr(name)
        .map_err(|_| PyTypeError::new_err(format!("{label} missing attribute: {name}")))?
        .extract::<i64>()
        .map_err(|_| PyTypeError::new_err(format!("{label}.{name} must be an integer")))
}

fn planner_ladder_rows_from_py_list(
    py: Python<'_>,
    list: &Bound<'_, PyList>,
) -> PyResult<Vec<PlannerLadderRow>> {
    let cls = planner_ladder_row_class(py)?;
    let mut entries = Vec::with_capacity(list.len());
    for (index, item) in list.iter().enumerate() {
        let label = format!("ladder_entries[{index}]");
        let item = require_instance(&item, &cls, &label, "PlannerLadderRow")?;
        entries.push(PlannerLadderRow {
            size_base_units: extract_i64(item, "size_base_units", &label)?,
            target_count: extract_i64(item, "target_count", &label)?,
            split_buffer_count: extract_i64(item, "split_buffer_count", &label)?,
        });
    }
    Ok(entries)
}

fn bootstrap_coins_from_py_list(
    py: Python<'_>,
    list: &Bound<'_, PyList>,
) -> PyResult<Vec<BootstrapCoin>> {
    let cls = bootstrap_coin_class(py)?;
    let mut coins = Vec::with_capacity(list.len());
    for (index, item) in list.iter().enumerate() {
        let label = format!("spendable_coins[{index}]");
        let item = require_instance(&item, &cls, &label, "BootstrapCoin")?;
        let id = item
            .getattr("id")
            .map_err(|_| PyTypeError::new_err(format!("{label} missing attribute: id")))?
            .extract::<String>()
            .map_err(|_| PyTypeError::new_err(format!("{label}.id must be a string")))?;
        coins.push(BootstrapCoin {
            id: id.trim().to_string(),
            amount: extract_i64(item, "amount", &label)?,
        });
    }
    Ok(coins)
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

fn bootstrap_plan_outcome_to_py<'py>(
    py: Python<'py>,
    outcome: BootstrapPlanOutcome,
) -> PyResult<Bound<'py, PyAny>> {
    let cls = bootstrap_plan_outcome_class(py)?;
    match outcome {
        BootstrapPlanOutcome::Ready => {
            let kwargs = PyDict::new(py);
            kwargs.set_item("kind", "ready")?;
            cls.call((), Some(&kwargs))
        }
        BootstrapPlanOutcome::NeedsSplit(plan) => {
            let kwargs = PyDict::new(py);
            kwargs.set_item("kind", "needs_split")?;
            kwargs.set_item("plan", bootstrap_plan_to_py(py, &plan)?)?;
            cls.call((), Some(&kwargs))
        }
        BootstrapPlanOutcome::CannotFund {
            total_output_amount,
        } => {
            let kwargs = PyDict::new(py);
            kwargs.set_item("kind", "cannot_fund")?;
            kwargs.set_item("total_output_amount", total_output_amount)?;
            cls.call((), Some(&kwargs))
        }
        BootstrapPlanOutcome::InvalidLadder => {
            let kwargs = PyDict::new(py);
            kwargs.set_item("kind", "invalid_ladder")?;
            cls.call((), Some(&kwargs))
        }
        BootstrapPlanOutcome::InvalidCoins => {
            let kwargs = PyDict::new(py);
            kwargs.set_item("kind", "invalid_coins")?;
            cls.call((), Some(&kwargs))
        }
    }
}

fn bootstrap_phase_result_to_py<'py>(
    py: Python<'py>,
    snapshot: BootstrapPhaseSnapshot,
) -> PyResult<Bound<'py, PyAny>> {
    let cls = bootstrap_phase_result_class(py)?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("status", snapshot.status)?;
    kwargs.set_item("reason", snapshot.reason)?;
    kwargs.set_item("ready", snapshot.ready)?;
    cls.call((), Some(&kwargs))
}

pub(crate) fn bootstrap_early_phase_from_py<'py>(
    py: Python<'py>,
    outcome_kind: &str,
    total_output_amount: i64,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    let snapshot = bootstrap_early_phase_from_kind(outcome_kind, total_output_amount);
    match snapshot {
        Some(snapshot) => Ok(Some(bootstrap_phase_result_to_py(py, snapshot)?)),
        None => Ok(None),
    }
}

pub(crate) fn bootstrap_executed_phase_from_py<'py>(
    py: Python<'py>,
    remaining_kind: &str,
    total_output_amount: i64,
) -> PyResult<Bound<'py, PyAny>> {
    let snapshot = bootstrap_executed_phase_from_kind(remaining_kind, total_output_amount);
    bootstrap_phase_result_to_py(py, snapshot)
}

pub(crate) fn plan_bootstrap_mixed_outputs_from_py<'py>(
    py: Python<'py>,
    ladder_entries: &Bound<'py, PyList>,
    spendable_coins: &Bound<'py, PyList>,
) -> PyResult<Bound<'py, PyAny>> {
    let ladder = planner_ladder_rows_from_py_list(py, ladder_entries)?;
    let coins = bootstrap_coins_from_py_list(py, spendable_coins)?;
    let outcome = plan_bootstrap_mixed_outputs(&ladder, &coins);
    bootstrap_plan_outcome_to_py(py, outcome)
}
