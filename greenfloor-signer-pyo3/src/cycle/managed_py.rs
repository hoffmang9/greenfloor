use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use crate::cycle::orchestration_py::parallel_action_outcomes_from_py_list;
use crate::py_utils::{dict_from_json_value, managed_retry_decision_class, request_dict_to_json, to_py_err};

use signer_core::{
    can_parallelize_managed_offers, classify_dexie_visibility_outcome, classify_managed_post_result,
    classify_managed_transient_error, count_parallel_transient_failures,
    is_managed_upstream_transient_error, is_managed_worker_transient_error,
    is_parallel_dispatch_transient_error, is_transient_dexie_visibility_404_error,
    is_transient_managed_upstream_error_text, managed_retry_decision,
    parallel_max_workers, prepare_parallel_managed_submission_decision,
    reservation_release_status, reservation_request_for_managed_offer, should_apply_parallel_transient_cooldown,
    single_input_preferred_skip_reason, SpendableAssetProfile,
};

#[pyfunction]
#[pyo3(name = "reservation_request_for_managed_offer")]
fn reservation_request_for_managed_offer_py(request: &Bound<'_, PyDict>) -> PyResult<Py<PyAny>> {
    let payload = request_dict_to_json(request)?;
    let side = payload
        .get("side")
        .and_then(|value| value.as_str())
        .unwrap_or("sell");
    let size_base_units = payload
        .get("size_base_units")
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    let base_asset_id = payload
        .get("base_asset_id")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let quote_asset_id = payload
        .get("quote_asset_id")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let base_unit_mojo_multiplier = payload
        .get("base_unit_mojo_multiplier")
        .and_then(|value| value.as_i64())
        .unwrap_or(1000);
    let quote_unit_mojo_multiplier = payload
        .get("quote_unit_mojo_multiplier")
        .and_then(|value| value.as_i64())
        .unwrap_or(1000);
    let quote_price = payload
        .get("quote_price")
        .and_then(|value| value.as_f64())
        .unwrap_or(0.0);
    let fee_asset_id = payload
        .get("fee_asset_id")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let fee_amount_mojos = payload
        .get("fee_amount_mojos")
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    let request_map = reservation_request_for_managed_offer(
        side,
        size_base_units,
        base_asset_id,
        quote_asset_id,
        base_unit_mojo_multiplier,
        quote_unit_mojo_multiplier,
        quote_price,
        fee_asset_id,
        fee_amount_mojos,
    );
    Python::attach(|py| {
        dict_from_json_value(py, serde_json::to_value(request_map).map_err(to_py_err)?)
    })
}

#[pyfunction]
#[pyo3(name = "single_input_preferred_skip_reason")]
fn single_input_preferred_skip_reason_py(
    requested_amounts: &Bound<'_, PyDict>,
    spendable_profiles: &Bound<'_, PyDict>,
) -> PyResult<Option<String>> {
    let requested_json = request_dict_to_json(requested_amounts)?;
    let profiles_json = request_dict_to_json(spendable_profiles)?;
    let requested: std::collections::BTreeMap<String, i64> =
        serde_json::from_value(requested_json).map_err(to_py_err)?;
    let profiles: std::collections::BTreeMap<String, SpendableAssetProfile> =
        serde_json::from_value(profiles_json).map_err(to_py_err)?;
    Ok(single_input_preferred_skip_reason(
        &requested,
        &profiles,
    ))
}

#[pyfunction]
#[pyo3(name = "is_transient_managed_upstream_error_text")]
fn is_transient_managed_upstream_error_text_py(error_text: &str) -> bool {
    is_transient_managed_upstream_error_text(error_text)
}

#[pyfunction]
#[pyo3(name = "classify_managed_transient_error")]
fn classify_managed_transient_error_py(
    exception_class: &str,
    error_text: &str,
) -> Option<String> {
    classify_managed_transient_error(exception_class, error_text)
}

#[pyfunction]
#[pyo3(name = "is_managed_upstream_transient_error")]
fn is_managed_upstream_transient_error_py(
    exception_class: &str,
    error_text: &str,
) -> bool {
    is_managed_upstream_transient_error(exception_class, error_text)
}

#[pyfunction]
#[pyo3(name = "is_managed_worker_transient_error")]
fn is_managed_worker_transient_error_py(exception_class: &str, error_text: &str) -> bool {
    is_managed_worker_transient_error(exception_class, error_text)
}

#[pyfunction]
#[pyo3(name = "is_parallel_dispatch_transient_error")]
fn is_parallel_dispatch_transient_error_py(exception_class: &str, error_text: &str) -> bool {
    is_parallel_dispatch_transient_error(exception_class, error_text)
}

#[pyfunction]
#[pyo3(name = "is_transient_dexie_visibility_404_error")]
fn is_transient_dexie_visibility_404_error_py(error: &str) -> bool {
    is_transient_dexie_visibility_404_error(error)
}

#[pyfunction]
#[pyo3(name = "can_parallelize_managed_offers")]
fn can_parallelize_managed_offers_py(
    signer_path_configured: bool,
    parallelism_enabled: bool,
    runtime_dry_run: bool,
    has_coordinator: bool,
) -> bool {
    can_parallelize_managed_offers(
        signer_path_configured,
        parallelism_enabled,
        runtime_dry_run,
        has_coordinator,
    )
}

#[pyfunction]
#[pyo3(name = "parallel_max_workers")]
fn parallel_max_workers_py(submission_count: usize, configured_max: usize) -> usize {
    parallel_max_workers(submission_count, configured_max)
}

#[pyfunction]
#[pyo3(name = "reservation_release_status")]
fn reservation_release_status_py(is_executed: bool) -> &'static str {
    reservation_release_status(is_executed)
}

#[pyfunction]
#[pyo3(name = "should_apply_parallel_transient_cooldown")]
fn should_apply_parallel_transient_cooldown_py(
    transient_failures: usize,
    total_parallel: usize,
    cooldown_seconds: u64,
) -> bool {
    should_apply_parallel_transient_cooldown(
        transient_failures,
        total_parallel,
        cooldown_seconds,
    )
}

#[pyfunction]
#[pyo3(name = "prepare_parallel_managed_submission_decision")]
fn prepare_parallel_managed_submission_decision_py(
    requested_amounts: &Bound<'_, PyDict>,
    spendable_profiles: &Bound<'_, PyDict>,
) -> PyResult<Py<PyAny>> {
    let requested_json = request_dict_to_json(requested_amounts)?;
    let profiles_json = request_dict_to_json(spendable_profiles)?;
    let requested: std::collections::BTreeMap<String, i64> =
        serde_json::from_value(requested_json).map_err(to_py_err)?;
    let profiles: std::collections::BTreeMap<String, SpendableAssetProfile> =
        serde_json::from_value(profiles_json).map_err(to_py_err)?;
    let decision = prepare_parallel_managed_submission_decision(&requested, &profiles);
    Python::attach(|py| {
        dict_from_json_value(py, serde_json::to_value(&decision).map_err(to_py_err)?)
    })
}

#[pyfunction]
#[pyo3(name = "classify_managed_post_result")]
fn classify_managed_post_result_py(
    success: bool,
    error_text: &str,
    offer_id: &str,
    publish_venue: &str,
) -> PyResult<Py<PyAny>> {
    let outcome = classify_managed_post_result(success, error_text, offer_id, publish_venue);
    Python::attach(|py| {
        dict_from_json_value(py, serde_json::to_value(&outcome).map_err(to_py_err)?)
    })
}

#[pyfunction]
#[pyo3(name = "classify_dexie_visibility_outcome")]
fn classify_dexie_visibility_outcome_py(visible: bool, visibility_error: &str) -> PyResult<Py<PyAny>> {
    let outcome = classify_dexie_visibility_outcome(visible, visibility_error);
    Python::attach(|py| {
        dict_from_json_value(py, serde_json::to_value(&outcome).map_err(to_py_err)?)
    })
}

#[pyfunction]
#[pyo3(name = "count_parallel_transient_failures")]
fn count_parallel_transient_failures_py(items: &Bound<'_, PyList>) -> PyResult<usize> {
    let pairs = parallel_action_outcomes_from_py_list(items)?;
    Ok(count_parallel_transient_failures(&pairs))
}

#[pyfunction]
#[pyo3(name = "managed_retry_decision")]
fn managed_retry_decision_py(
    attempt_index: u32,
    attempts_max: u32,
    backoff_ms: u64,
    is_upstream_transient: bool,
) -> PyResult<Py<PyAny>> {
    let decision = managed_retry_decision(
        attempt_index,
        attempts_max,
        backoff_ms,
        is_upstream_transient,
    );
    let decision_label = match decision.decision {
        signer_core::ManagedRetryDecisionKind::Stop => "stop",
        signer_core::ManagedRetryDecisionKind::Retry => "retry",
    };
    Python::attach(|py| {
        let cls = managed_retry_decision_class(py)?;
        let kwargs = PyDict::new(py);
        kwargs.set_item("decision", decision_label)?;
        kwargs.set_item("sleep_ms", decision.sleep_ms)?;
        Ok(cls.call((), Some(&kwargs))?.into())
    })
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(
        reservation_request_for_managed_offer_py,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(single_input_preferred_skip_reason_py, m)?)?;
    m.add_function(wrap_pyfunction!(
        is_transient_managed_upstream_error_text_py,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(classify_managed_transient_error_py, m)?)?;
    m.add_function(wrap_pyfunction!(is_managed_upstream_transient_error_py, m)?)?;
    m.add_function(wrap_pyfunction!(is_managed_worker_transient_error_py, m)?)?;
    m.add_function(wrap_pyfunction!(is_parallel_dispatch_transient_error_py, m)?)?;
    m.add_function(wrap_pyfunction!(is_transient_dexie_visibility_404_error_py, m)?)?;
    m.add_function(wrap_pyfunction!(can_parallelize_managed_offers_py, m)?)?;
    m.add_function(wrap_pyfunction!(parallel_max_workers_py, m)?)?;
    m.add_function(wrap_pyfunction!(reservation_release_status_py, m)?)?;
    m.add_function(wrap_pyfunction!(
        should_apply_parallel_transient_cooldown_py,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        prepare_parallel_managed_submission_decision_py,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(classify_managed_post_result_py, m)?)?;
    m.add_function(wrap_pyfunction!(classify_dexie_visibility_outcome_py, m)?)?;
    m.add_function(wrap_pyfunction!(count_parallel_transient_failures_py, m)?)?;
    m.add_function(wrap_pyfunction!(managed_retry_decision_py, m)?)?;
    Ok(())
}
