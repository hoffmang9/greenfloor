use std::collections::BTreeMap;
use std::sync::OnceLock;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use super::common::{cached_class, i64_i64_map_to_py_dict};

const ORCHESTRATION_MODULE: &str = "greenfloor.core.cycle_orchestration";
const CYCLE_RESEED_MODULE: &str = "greenfloor.core.cycle_reseed";
const OFFER_RECONCILE_MODULE: &str = "greenfloor.core.offer_reconcile";

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

pub fn planned_action_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &PLANNED_ACTION_CLS,
        "greenfloor.core.planned_action",
        "PlannedAction",
    )
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
    cached_class(
        py,
        &RESEED_GAP_PLAN_CLS,
        CYCLE_RESEED_MODULE,
        "ReseedGapPlan",
    )
}

pub fn reseed_skip_reason_to_py<'py>(
    py: Python<'py>,
    reason: signer_core::ReseedSkipReason,
) -> PyResult<Bound<'py, PyAny>> {
    let cls = cached_class(
        py,
        &RESEED_SKIP_REASON_CLS,
        CYCLE_RESEED_MODULE,
        "ReseedSkipReason",
    )?;
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
    kwargs.set_item(
        "missing_by_size",
        i64_i64_map_to_py_dict(py, &plan.missing_by_size)?,
    )?;
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
    cached_class(
        py,
        &MARKET_BATCH_SELECTION_CLS,
        ORCHESTRATION_MODULE,
        "MarketBatchSelection",
    )
}

pub fn offer_state_row_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &OFFER_STATE_ROW_CLS,
        ORCHESTRATION_MODULE,
        "OfferStateRow",
    )
}

pub fn stale_sweep_candidate_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &STALE_SWEEP_CANDIDATE_CLS,
        ORCHESTRATION_MODULE,
        "StaleSweepCandidate",
    )
}

pub fn stale_sweep_hit_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &STALE_SWEEP_HIT_CLS,
        ORCHESTRATION_MODULE,
        "StaleSweepHit",
    )
}

pub fn stale_sweep_progress_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &STALE_SWEEP_PROGRESS_CLS,
        ORCHESTRATION_MODULE,
        "StaleSweepProgress",
    )
}

pub fn cycle_offer_transition_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &CYCLE_OFFER_TRANSITION_CLS,
        OFFER_RECONCILE_MODULE,
        "CycleOfferTransition",
    )
}

pub fn extract_spendable_profiles(
    profiles: &Bound<'_, PyDict>,
) -> PyResult<BTreeMap<String, signer_core::SpendableAssetProfile>> {
    let mut map = BTreeMap::new();
    for (asset_id, value) in profiles.iter() {
        let profile = value
            .downcast::<PyDict>()
            .map_err(|_| PyValueError::new_err("spendable profile values must be dicts"))?;
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
