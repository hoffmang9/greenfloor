use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use crate::py_utils::{
    dict_from_json_value, dict_to_i64_i64_map, i64_i64_map_to_py_dict, request_dict_to_json,
    to_py_err,
};

use signer_core::{
    apply_offer_signal, can_parallelize_managed_offers, classify_dexie_stale_offer_status,
    classify_dexie_visibility_outcome, classify_managed_post_result, classify_managed_transient_error,
    collect_stale_sweep_candidates, count_parallel_transient_failures, dedupe_sorted_market_ids,
    enqueue_immediate_requeue, expiry_seconds_for_action, is_dexie_offer_missing_error_text,
    is_managed_upstream_transient_error, is_managed_worker_transient_error,
    is_parallel_dispatch_transient_error, is_transient_dexie_visibility_404_error,
    is_transient_managed_upstream_error_text, is_two_sided_market_mode, managed_retry_sleep_ms,
    market_cycle_phases, needs_inventory_fallback, next_disabled_market_log_deadline,
    one_sided_offer_counts_by_side, parallel_max_workers, prepare_parallel_managed_submission_decision,
    record_stale_sweep_check, reservation_release_status, reservation_request_for_managed_offer,
    resolve_inventory_scan_source, resolve_tracked_sizes, select_market_batch,
    should_apply_parallel_transient_cooldown, should_log_disabled_market, should_retry_managed_post,
    should_try_cat_inventory_fallback, should_use_market_slot_dispatch,
    single_input_preferred_skip_reason, aggregate_two_sided_offer_counts, OfferLifecycleState,
    OfferSignal, OfferStateRow, SpendableAssetProfile, StaleSweepProgress,
};

#[pyfunction]
#[pyo3(name = "apply_offer_signal")]
fn apply_offer_signal_py(state: &str, signal: &str) -> PyResult<Py<PyAny>> {
    let state = parse_lifecycle_state(state)?;
    let signal = parse_offer_signal(signal)?;
    let transition = apply_offer_signal(state, signal);
    Python::attach(|py| {
        dict_from_json_value(
            py,
            serde_json::to_value(&transition).map_err(to_py_err)?,
        )
    })
}

fn parse_lifecycle_state(value: &str) -> PyResult<OfferLifecycleState> {
    match value.trim() {
        "open" => Ok(OfferLifecycleState::Open),
        "mempool_observed" => Ok(OfferLifecycleState::MempoolObserved),
        "tx_block_confirmed" => Ok(OfferLifecycleState::TxBlockConfirmed),
        "refresh_due" => Ok(OfferLifecycleState::RefreshDue),
        "expired" => Ok(OfferLifecycleState::Expired),
        other => Err(PyValueError::new_err(format!(
            "unknown offer lifecycle state: {other}"
        ))),
    }
}

fn parse_offer_signal(value: &str) -> PyResult<OfferSignal> {
    match value.trim() {
        "mempool_seen" => Ok(OfferSignal::MempoolSeen),
        "tx_confirmed" => Ok(OfferSignal::TxConfirmed),
        "expiry_near" => Ok(OfferSignal::ExpiryNear),
        "expired" => Ok(OfferSignal::Expired),
        "refresh_posted" => Ok(OfferSignal::RefreshPosted),
        other => Err(PyValueError::new_err(format!("unknown offer signal: {other}"))),
    }
}

#[pyfunction]
#[pyo3(name = "expiry_seconds_for_action")]
fn expiry_seconds_for_action_py(expiry_unit: &str, expiry_value: i64) -> PyResult<Option<i64>> {
    Ok(expiry_seconds_for_action(expiry_unit, expiry_value))
}

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
#[pyo3(name = "managed_retry_sleep_ms")]
fn managed_retry_sleep_ms_py(attempt_index: u32, backoff_ms: u64) -> u64 {
    managed_retry_sleep_ms(attempt_index, backoff_ms)
}

#[pyfunction]
#[pyo3(name = "should_retry_managed_post")]
fn should_retry_managed_post_py(
    attempt_index: u32,
    attempts_max: u32,
    is_upstream_transient: bool,
) -> bool {
    should_retry_managed_post(attempt_index, attempts_max, is_upstream_transient)
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
    let mut pairs = Vec::new();
    for item in items.iter() {
        let dict = item.cast::<PyDict>()?;
        let status: String = dict.get_item("status")?.unwrap().extract()?;
        let transient_upstream: bool = dict
            .get_item("transient_upstream")?
            .map(|value| value.extract())
            .transpose()?
            .unwrap_or(false);
        pairs.push((status, transient_upstream));
    }
    Ok(count_parallel_transient_failures(&pairs))
}

#[pyfunction]
#[pyo3(name = "select_market_batch")]
fn select_market_batch_py(
    enabled_market_ids: Vec<String>,
    slot_count: usize,
    cursor: usize,
    immediate_requeue_ids: Vec<String>,
) -> PyResult<Py<PyAny>> {
    let selection = select_market_batch(
        &enabled_market_ids,
        slot_count,
        cursor,
        &immediate_requeue_ids,
    );
    Python::attach(|py| {
        dict_from_json_value(py, serde_json::to_value(&selection).map_err(to_py_err)?)
    })
}

#[pyfunction]
#[pyo3(name = "enqueue_immediate_requeue")]
fn enqueue_immediate_requeue_py(
    immediate_requeue_ids: Vec<String>,
    market_id: &str,
) -> Vec<String> {
    enqueue_immediate_requeue(&immediate_requeue_ids, market_id)
}

#[pyfunction]
#[pyo3(name = "should_use_market_slot_dispatch")]
fn should_use_market_slot_dispatch_py(enabled_market_count: usize, slot_count: usize) -> bool {
    should_use_market_slot_dispatch(enabled_market_count, slot_count)
}

#[pyfunction]
#[pyo3(name = "dedupe_sorted_market_ids")]
fn dedupe_sorted_market_ids_py(market_ids: Vec<String>) -> Vec<String> {
    dedupe_sorted_market_ids(&market_ids)
}

#[pyfunction]
#[pyo3(name = "should_log_disabled_market")]
fn should_log_disabled_market_py(now_monotonic: f64, next_log_deadline: f64) -> bool {
    should_log_disabled_market(now_monotonic, next_log_deadline)
}

#[pyfunction]
#[pyo3(name = "next_disabled_market_log_deadline")]
fn next_disabled_market_log_deadline_py(now_monotonic: f64, interval_seconds: u64) -> f64 {
    next_disabled_market_log_deadline(now_monotonic, interval_seconds)
}

#[pyfunction]
#[pyo3(name = "should_try_cat_inventory_fallback")]
fn should_try_cat_inventory_fallback_py(coinset_scan_empty: bool, base_asset: &str) -> bool {
    should_try_cat_inventory_fallback(coinset_scan_empty, base_asset)
}

#[pyfunction]
#[pyo3(name = "collect_stale_sweep_candidates")]
fn collect_stale_sweep_candidates_py(
    rows: &Bound<'_, PyList>,
    enabled_market_ids: Vec<String>,
    per_market_limit: usize,
) -> PyResult<Py<PyAny>> {
    let mut offer_rows = Vec::new();
    for item in rows.iter() {
        let dict = item.cast::<PyDict>()?;
        let payload = request_dict_to_json(&dict)?;
        offer_rows.push(serde_json::from_value::<OfferStateRow>(payload).map_err(to_py_err)?);
    }
    let candidates =
        collect_stale_sweep_candidates(&offer_rows, &enabled_market_ids, per_market_limit);
    Python::attach(|py| {
        dict_from_json_value(py, serde_json::to_value(candidates).map_err(to_py_err)?)
    })
}

#[pyfunction]
#[pyo3(name = "classify_dexie_stale_offer_status")]
fn classify_dexie_stale_offer_status_py(status: i64) -> Option<String> {
    classify_dexie_stale_offer_status(status).map(str::to_string)
}

#[pyfunction]
#[pyo3(name = "is_dexie_offer_missing_error_text")]
fn is_dexie_offer_missing_error_text_py(error_text: &str) -> bool {
    is_dexie_offer_missing_error_text(error_text)
}

#[pyfunction]
#[pyo3(name = "record_stale_sweep_check")]
fn record_stale_sweep_check_py(
    progress: &Bound<'_, PyDict>,
    hit: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    let progress_json = request_dict_to_json(progress)?;
    let mut current: StaleSweepProgress =
        serde_json::from_value(progress_json).map_err(to_py_err)?;
    let hit_value = if let Some(hit_dict) = hit {
        let hit_json = request_dict_to_json(hit_dict)?;
        Some(serde_json::from_value(hit_json).map_err(to_py_err)?)
    } else {
        None
    };
    current = record_stale_sweep_check(&current, hit_value);
    Python::attach(|py| {
        dict_from_json_value(py, serde_json::to_value(&current).map_err(to_py_err)?)
    })
}

#[pyfunction]
#[pyo3(name = "market_cycle_phases")]
fn market_cycle_phases_py() -> Vec<String> {
    market_cycle_phases()
        .iter()
        .map(|phase| phase.as_str().to_string())
        .collect()
}

#[pyfunction]
#[pyo3(name = "needs_inventory_fallback")]
fn needs_inventory_fallback_py(bucket_counts_available: bool, coinset_scan_empty: bool) -> bool {
    needs_inventory_fallback(bucket_counts_available, coinset_scan_empty)
}

#[pyfunction]
#[pyo3(name = "resolve_inventory_scan_source")]
fn resolve_inventory_scan_source_py(
    coinset_scan_found_coins: bool,
    coinset_scan_empty: bool,
    cat_scan_found_coins: bool,
    wallet_scan_found_coins: bool,
) -> &'static str {
    resolve_inventory_scan_source(
        coinset_scan_found_coins,
        coinset_scan_empty,
        cat_scan_found_coins,
        wallet_scan_found_coins,
    )
}

#[pyfunction]
#[pyo3(name = "resolve_tracked_sizes")]
fn resolve_tracked_sizes_py(ladder_sizes: Vec<i64>, strategy_default_sizes: Vec<i64>) -> Vec<i64> {
    resolve_tracked_sizes(&ladder_sizes, &strategy_default_sizes)
}

#[pyfunction]
#[pyo3(name = "is_two_sided_market_mode")]
fn is_two_sided_market_mode_py(market_mode: &str) -> bool {
    is_two_sided_market_mode(market_mode)
}

#[pyfunction]
#[pyo3(name = "aggregate_two_sided_offer_counts")]
fn aggregate_two_sided_offer_counts_py(
    buy_counts: &Bound<'_, PyDict>,
    sell_counts: &Bound<'_, PyDict>,
    tracked_sizes: Vec<i64>,
) -> PyResult<Py<PyAny>> {
    let buy = dict_to_i64_i64_map(buy_counts)?;
    let sell = dict_to_i64_i64_map(sell_counts)?;
    let aggregated = aggregate_two_sided_offer_counts(&buy, &sell, &tracked_sizes);
    Python::attach(|py| Ok(i64_i64_map_to_py_dict(py, &aggregated)?.into()))
}

#[pyfunction]
#[pyo3(name = "one_sided_offer_counts_by_side")]
fn one_sided_offer_counts_by_side_py(
    sell_counts: &Bound<'_, PyDict>,
    tracked_sizes: Vec<i64>,
) -> PyResult<Py<PyAny>> {
    let sell = dict_to_i64_i64_map(sell_counts)?;
    let (buy, sell_side) = one_sided_offer_counts_by_side(&sell, &tracked_sizes);
    Python::attach(|py| {
        let dict = PyDict::new(py);
        dict.set_item("buy", i64_i64_map_to_py_dict(py, &buy)?)?;
        dict.set_item("sell", i64_i64_map_to_py_dict(py, &sell_side)?)?;
        Ok(dict.into())
    })
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    crate::strategy_py::register_strategy(m)?;
    crate::execution_py::register_execution(m)?;
    m.add_function(wrap_pyfunction!(apply_offer_signal_py, m)?)?;
    m.add_function(wrap_pyfunction!(expiry_seconds_for_action_py, m)?)?;
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
    m.add_function(wrap_pyfunction!(managed_retry_sleep_ms_py, m)?)?;
    m.add_function(wrap_pyfunction!(should_retry_managed_post_py, m)?)?;
    m.add_function(wrap_pyfunction!(
        prepare_parallel_managed_submission_decision_py,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(classify_managed_post_result_py, m)?)?;
    m.add_function(wrap_pyfunction!(classify_dexie_visibility_outcome_py, m)?)?;
    m.add_function(wrap_pyfunction!(count_parallel_transient_failures_py, m)?)?;
    m.add_function(wrap_pyfunction!(select_market_batch_py, m)?)?;
    m.add_function(wrap_pyfunction!(enqueue_immediate_requeue_py, m)?)?;
    m.add_function(wrap_pyfunction!(should_use_market_slot_dispatch_py, m)?)?;
    m.add_function(wrap_pyfunction!(dedupe_sorted_market_ids_py, m)?)?;
    m.add_function(wrap_pyfunction!(should_log_disabled_market_py, m)?)?;
    m.add_function(wrap_pyfunction!(next_disabled_market_log_deadline_py, m)?)?;
    m.add_function(wrap_pyfunction!(should_try_cat_inventory_fallback_py, m)?)?;
    m.add_function(wrap_pyfunction!(collect_stale_sweep_candidates_py, m)?)?;
    m.add_function(wrap_pyfunction!(classify_dexie_stale_offer_status_py, m)?)?;
    m.add_function(wrap_pyfunction!(is_dexie_offer_missing_error_text_py, m)?)?;
    m.add_function(wrap_pyfunction!(record_stale_sweep_check_py, m)?)?;
    m.add_function(wrap_pyfunction!(market_cycle_phases_py, m)?)?;
    m.add_function(wrap_pyfunction!(needs_inventory_fallback_py, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_inventory_scan_source_py, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_tracked_sizes_py, m)?)?;
    m.add_function(wrap_pyfunction!(is_two_sided_market_mode_py, m)?)?;
    m.add_function(wrap_pyfunction!(aggregate_two_sided_offer_counts_py, m)?)?;
    m.add_function(wrap_pyfunction!(one_sided_offer_counts_by_side_py, m)?)?;
    Ok(())
}
