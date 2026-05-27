extern crate greenfloor_signer as signer_core;

use std::future::Future;
use std::path::Path;
use std::sync::OnceLock;

use std::collections::BTreeMap;

use chia_bls::SecretKey;
use signer_core::{
    apply_offer_signal, broadcast_bls_spend_bundle, build_and_optionally_broadcast_vault_cat_mixed_split,
    build_bls_mixed_split_spend_bundle, build_bls_offer_spend_bundle,
    build_bls_xch_coin_op_spend_bundle, build_vault_cat_offer, can_parallelize_managed_offers,
    classify_dexie_stale_offer_status, classify_dexie_visibility_outcome,
    classify_managed_post_result, classify_managed_transient_error, collect_stale_sweep_candidates,
    count_parallel_transient_failures, dedupe_sorted_market_ids, enqueue_immediate_requeue,
    encode_offer_from_spend_bundle_bytes, evaluate_market, expand_strategy_actions, expiry_seconds_for_action,
    from_input_spend_bundle_bytes, from_input_spend_bundle_xch_bytes, get_conservative_fee_estimate,
    get_fee_estimate, is_dexie_offer_missing_error_text, is_managed_upstream_transient_error,
    is_managed_worker_transient_error, is_parallel_dispatch_transient_error,
    is_transient_dexie_visibility_404_error, is_transient_managed_upstream_error_text,
    list_cat_coin_summaries, list_cat_coin_summaries_by_ids, load_bls_master_secret_key,
    load_signer_config, managed_retry_sleep_ms, next_disabled_market_log_deadline,
    parallel_max_workers, prepare_parallel_managed_submission_decision, push_tx_hex,
    record_stale_sweep_check, reservation_release_status, reservation_request_for_managed_offer,
    resolve_offer_asset_ids, resolve_vault_context, select_market_batch,
    should_apply_parallel_transient_cooldown, should_log_disabled_market, should_retry_managed_post,
    should_try_cat_inventory_fallback, should_use_market_slot_dispatch,
    single_input_preferred_skip_reason, validate_offer_text, BlsMixedSplitRequest, BlsOfferRequest,
    BlsXchCoinOpRequest, CreateOfferRequest, MarketCycleResultState,
    MarketState, OfferLifecycleState, OfferSignal, OfferStateRow, PlannedActionInput,
    SpendableAssetProfile, StaleSweepProgress, StrategyConfig, MixedSplitRequest,
    aggregate_two_sided_offer_counts, is_two_sided_market_mode, market_cycle_phases,
    needs_inventory_fallback, one_sided_offer_counts_by_side, resolve_inventory_scan_source,
    resolve_tracked_sizes,
};
use signer_core::error::{bls_reason, broadcast_reason, BlsOp};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyModule};

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

fn to_py_err<E: std::fmt::Display>(err: E) -> PyErr {
    PyValueError::new_err(err.to_string())
}

fn dict_from_json_value(py: Python<'_>, value: serde_json::Value) -> PyResult<Py<PyAny>> {
    let json = serde_json::to_string(&value).map_err(to_py_err)?;
    let builtins = py.import("json")?;
    let loads = builtins.getattr("loads")?;
    let obj = loads.call1((json,))?;
    Ok(obj.unbind())
}

fn request_dict_to_json(request: &Bound<'_, PyDict>) -> PyResult<serde_json::Value> {
    let py = request.py();
    let json_mod = py.import("json")?;
    let dumps = json_mod.getattr("dumps")?;
    let raw = dumps.call1((request,))?;
    let raw_str: String = raw.extract()?;
    serde_json::from_str(&raw_str).map_err(to_py_err)
}

fn parse_master_sk_bytes(master_sk_bytes: &[u8]) -> PyResult<SecretKey> {
    let bytes: [u8; 32] = master_sk_bytes
        .try_into()
        .map_err(|_| PyValueError::new_err("master_sk must be exactly 32 bytes"))?;
    SecretKey::from_bytes(&bytes).map_err(to_py_err)
}

fn block_on_signer<F, T>(future: F) -> Result<T, signer_core::Error>
where
    F: Future<Output = Result<T, signer_core::Error>>,
{
    runtime().block_on(future)
}

fn bls_build_dict_py<T>(
    py: Python<'_>,
    op: BlsOp,
    result: Result<T, signer_core::Error>,
    fill_ok: impl FnOnce(&Bound<'_, PyDict>, T) -> PyResult<()>,
) -> PyResult<Py<PyAny>> {
    let dict = PyDict::new(py);
    match result {
        Ok(value) => fill_ok(&dict, value)?,
        Err(err) => {
            dict.set_item("error", bls_reason(err, op))?;
        }
    }
    Ok(dict.into())
}

#[pyfunction]
#[pyo3(name = "resolve_vault_context")]
fn resolve_vault_context_py(config_path: &str) -> PyResult<Py<PyAny>> {
    let config = load_signer_config(Path::new(config_path)).map_err(to_py_err)?;
    let context = runtime()
        .block_on(resolve_vault_context(config))
        .map_err(to_py_err)?;
    Python::attach(|py| {
        dict_from_json_value(
            py,
            serde_json::to_value(&context).map_err(to_py_err)?,
        )
    })
}

#[pyfunction]
#[pyo3(name = "build_vault_cat_offer")]
fn build_vault_cat_offer_py(config_path: &str, request: &Bound<'_, PyDict>) -> PyResult<Py<PyAny>> {
    let config = load_signer_config(Path::new(config_path)).map_err(to_py_err)?;
    let payload = request_dict_to_json(request)?;
    let offer_request: CreateOfferRequest =
        serde_json::from_value(payload).map_err(to_py_err)?;
    let result = runtime()
        .block_on(build_vault_cat_offer(config, offer_request))
        .map_err(to_py_err)?;
    Python::attach(|py| {
        dict_from_json_value(
            py,
            serde_json::to_value(&result).map_err(to_py_err)?,
        )
    })
}

#[pyfunction]
#[pyo3(name = "build_mixed_split")]
fn build_mixed_split_py(config_path: &str, request: &Bound<'_, PyDict>) -> PyResult<Py<PyAny>> {
    let config = load_signer_config(Path::new(config_path)).map_err(to_py_err)?;
    let payload = request_dict_to_json(request)?;
    let broadcast = payload
        .get("broadcast")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let split_request: MixedSplitRequest =
        serde_json::from_value(payload).map_err(to_py_err)?;
    let result = runtime()
        .block_on(build_and_optionally_broadcast_vault_cat_mixed_split(
            config,
            split_request,
            broadcast,
        ))
        .map_err(to_py_err)?;
    Python::attach(|py| {
        dict_from_json_value(
            py,
            serde_json::to_value(&result).map_err(to_py_err)?,
        )
    })
}

#[pyfunction]
#[pyo3(name = "build_bls_mixed_split")]
fn build_bls_mixed_split_py(
    network: &str,
    master_sk_bytes: &[u8],
    request: &Bound<'_, PyDict>,
) -> PyResult<Py<PyAny>> {
    Python::attach(|py| {
        let master_sk = parse_master_sk_bytes(master_sk_bytes)?;
        let payload = request_dict_to_json(request)?;
        let split_request: BlsMixedSplitRequest =
            serde_json::from_value(payload).map_err(to_py_err)?;
        let result = block_on_signer(build_bls_mixed_split_spend_bundle(
            network,
            &master_sk,
            split_request,
        ));
        bls_build_dict_py(py, BlsOp::MixedSplit, result, |dict, built| {
            dict.set_item("spend_bundle_hex", built.spend_bundle_hex)?;
            if !built.selected_coin_ids.is_empty() {
                dict.set_item("selected_coin_ids", built.selected_coin_ids)?;
            }
            Ok(())
        })
    })
}

#[pyfunction]
#[pyo3(name = "build_bls_offer")]
fn build_bls_offer_py(
    network: &str,
    master_sk_bytes: &[u8],
    request: &Bound<'_, PyDict>,
) -> PyResult<Py<PyAny>> {
    Python::attach(|py| {
        let master_sk = parse_master_sk_bytes(master_sk_bytes)?;
        let payload = request_dict_to_json(request)?;
        let offer_request: BlsOfferRequest = serde_json::from_value(payload).map_err(to_py_err)?;
        let result = block_on_signer(build_bls_offer_spend_bundle(
            network,
            &master_sk,
            offer_request,
        ));
        bls_build_dict_py(py, BlsOp::Offer, result, |dict, built| {
            dict.set_item("spend_bundle_hex", built.spend_bundle_hex)?;
            Ok(())
        })
    })
}

#[pyfunction]
#[pyo3(name = "build_bls_xch_coin_op")]
fn build_bls_xch_coin_op_py(
    network: &str,
    master_sk_bytes: &[u8],
    request: &Bound<'_, PyDict>,
) -> PyResult<Py<PyAny>> {
    Python::attach(|py| {
        let master_sk = parse_master_sk_bytes(master_sk_bytes)?;
        let payload = request_dict_to_json(request)?;
        let coin_op_request: BlsXchCoinOpRequest =
            serde_json::from_value(payload).map_err(to_py_err)?;
        let result = block_on_signer(build_bls_xch_coin_op_spend_bundle(
            network,
            &master_sk,
            coin_op_request,
        ));
        bls_build_dict_py(py, BlsOp::XchCoinOp, result, |dict, built| {
            dict.set_item("spend_bundle_hex", built.spend_bundle_hex)?;
            Ok(())
        })
    })
}

#[pyfunction]
#[pyo3(name = "list_bls_cat_coins")]
fn list_bls_cat_coins_py(
    network: &str,
    receive_address: &str,
    asset_id: &str,
) -> PyResult<Py<PyAny>> {
    Python::attach(|py| {
        let summaries = runtime()
            .block_on(list_cat_coin_summaries(network, receive_address, asset_id))
            .map_err(to_py_err)?;
        summaries_to_py_list(py, summaries)
    })
}

#[pyfunction]
#[pyo3(name = "list_bls_cat_coins_by_ids")]
fn list_bls_cat_coins_by_ids_py(network: &str, coin_ids: Vec<String>) -> PyResult<Py<PyAny>> {
    Python::attach(|py| {
        let summaries = runtime()
            .block_on(list_cat_coin_summaries_by_ids(network, &coin_ids))
            .map_err(to_py_err)?;
        summaries_to_py_list(py, summaries)
    })
}

fn summaries_to_py_list(
    py: Python<'_>,
    summaries: Vec<signer_core::CoinRecordSummary>,
) -> PyResult<Py<PyAny>> {
    let list = PyList::empty(py);
    for summary in summaries {
        let dict = PyDict::new(py);
        dict.set_item("coin_id", summary.coin_id)?;
        dict.set_item("parent_coin_info", summary.parent_coin_info)?;
        dict.set_item("puzzle_hash", summary.puzzle_hash)?;
        dict.set_item("amount", summary.amount)?;
        if let Some(p2) = summary.p2_puzzle_hash {
            dict.set_item("p2_puzzle_hash", p2)?;
        }
        if let Some(asset_id) = summary.asset_id {
            dict.set_item("asset_id", asset_id)?;
        }
        list.append(dict)?;
    }
    Ok(list.into())
}

#[pyfunction]
#[pyo3(name = "broadcast_bls_spend_bundle")]
fn broadcast_bls_spend_bundle_py(network: &str, spend_bundle_hex: &str) -> PyResult<Py<PyAny>> {
    Python::attach(|py| {
        let dict = PyDict::new(py);
        match runtime().block_on(broadcast_bls_spend_bundle(network, spend_bundle_hex)) {
            Ok(result) => {
                dict.set_item("status", "executed")?;
                dict.set_item("reason", result.status)?;
                dict.set_item("operation_id", result.operation_id)?;
            }
            Err(err) => {
                dict.set_item("status", "skipped")?;
                dict.set_item("reason", broadcast_reason(err))?;
                dict.set_item("operation_id", py.None())?;
            }
        }
        Ok(dict.into())
    })
}

#[pyfunction]
#[pyo3(name = "resolve_offer_asset_ids")]
fn resolve_offer_asset_ids_py(
    config_path: &str,
    base_asset: &str,
    quote_asset: &str,
) -> PyResult<Py<PyAny>> {
    let config = load_signer_config(Path::new(config_path)).map_err(to_py_err)?;
    let (base, quote) = runtime()
        .block_on(resolve_offer_asset_ids(config, base_asset, quote_asset))
        .map_err(to_py_err)?;
    Python::attach(|py| {
        let dict = PyDict::new(py);
        dict.set_item("base_asset_id", base)?;
        dict.set_item("quote_asset_id", quote)?;
        Ok(dict.into())
    })
}

#[pyfunction]
#[pyo3(name = "validate_offer")]
fn validate_offer_py(offer: &str) -> PyResult<()> {
    validate_offer_text(offer).map_err(to_py_err)
}

#[pyfunction]
#[pyo3(name = "encode_offer")]
fn encode_offer_py(spend_bundle_bytes: &[u8]) -> PyResult<String> {
    encode_offer_from_spend_bundle_bytes(spend_bundle_bytes).map_err(to_py_err)
}

#[pyfunction]
#[pyo3(name = "from_input_spend_bundle")]
fn from_input_spend_bundle_py(
    spend_bundle_bytes: &[u8],
    requested_payments_xch: Vec<(Vec<u8>, Vec<(Vec<u8>, u64)>)>,
    requested_payments_cats: Vec<(Vec<u8>, Vec<u8>, Vec<(Vec<u8>, u64)>)>,
) -> PyResult<Vec<u8>> {
    from_input_spend_bundle_bytes(
        spend_bundle_bytes,
        requested_payments_xch,
        requested_payments_cats,
    )
    .map_err(to_py_err)
}

#[pyfunction]
#[pyo3(name = "from_input_spend_bundle_xch")]
fn from_input_spend_bundle_xch_py(
    spend_bundle_bytes: &[u8],
    requested_payments_xch: Vec<(Vec<u8>, Vec<(Vec<u8>, u64)>)>,
) -> PyResult<Vec<u8>> {
    from_input_spend_bundle_xch_bytes(spend_bundle_bytes, requested_payments_xch).map_err(to_py_err)
}

#[pyfunction]
#[pyo3(name = "load_bls_master_sk")]
fn load_bls_master_sk_py(key_id: &str) -> PyResult<Py<PyAny>> {
    Python::attach(|py| {
        let dict = PyDict::new(py);
        match load_bls_master_secret_key(key_id) {
            Ok(sk) => {
                dict.set_item("master_sk_bytes", sk.to_bytes().to_vec())?;
            }
            Err(err) => {
                dict.set_item("error", bls_reason(err, BlsOp::KeyLoad))?;
            }
        }
        Ok(dict.into())
    })
}

#[pyfunction]
#[pyo3(name = "coinset_push_tx")]
fn coinset_push_tx_py(
    network: &str,
    base_url: &str,
    spend_bundle_hex: &str,
) -> PyResult<Py<PyAny>> {
    let base = base_url.trim();
    let base_opt = if base.is_empty() { None } else { Some(base) };
    let payload = runtime()
        .block_on(push_tx_hex(network, base_opt, spend_bundle_hex))
        .map_err(to_py_err)?;
    Python::attach(|py| dict_from_json_value(py, payload))
}

#[pyfunction]
#[pyo3(name = "coinset_get_fee_estimate")]
fn coinset_get_fee_estimate_py(
    network: &str,
    base_url: &str,
    target_times: Vec<u64>,
    cost: u64,
    spend_count: Option<u64>,
) -> PyResult<Py<PyAny>> {
    let base = base_url.trim();
    let base_opt = if base.is_empty() { None } else { Some(base) };
    let payload = runtime()
        .block_on(get_fee_estimate(
            network,
            base_opt,
            target_times,
            cost,
            spend_count,
        ))
        .map_err(to_py_err)?;
    Python::attach(|py| dict_from_json_value(py, payload))
}

#[pyfunction]
#[pyo3(name = "coinset_get_conservative_fee_estimate")]
fn coinset_get_conservative_fee_estimate_py(
    network: &str,
    base_url: &str,
    cost: u64,
    spend_count: Option<u64>,
) -> PyResult<Option<u64>> {
    let base = base_url.trim();
    let base_opt = if base.is_empty() { None } else { Some(base) };
    runtime()
        .block_on(get_conservative_fee_estimate(
            network,
            base_opt,
            cost,
            spend_count,
        ))
        .map_err(to_py_err)
}

#[pyfunction]
#[pyo3(name = "evaluate_market")]
fn evaluate_market_py(state: &Bound<'_, PyDict>, config: &Bound<'_, PyDict>) -> PyResult<Py<PyAny>> {
    let state: MarketState = request_dict_to_json(state)
        .and_then(|value| serde_json::from_value(value).map_err(to_py_err))?;
    let config: StrategyConfig = request_dict_to_json(config)
        .and_then(|value| serde_json::from_value(value).map_err(to_py_err))?;
    let actions = evaluate_market(&state, &config);
    Python::attach(|py| {
        dict_from_json_value(py, serde_json::to_value(actions).map_err(to_py_err)?)
    })
}

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
#[pyo3(name = "expand_strategy_actions")]
fn expand_strategy_actions_py(actions: &Bound<'_, PyList>) -> PyResult<Py<PyAny>> {
    let mut inputs = Vec::new();
    for item in actions.iter() {
        let dict = item.cast::<PyDict>()?;
        let payload = request_dict_to_json(&dict)?;
        inputs.push(
            serde_json::from_value::<PlannedActionInput>(payload).map_err(to_py_err)?,
        );
    }
    let expanded = expand_strategy_actions(&inputs);
    Python::attach(|py| {
        dict_from_json_value(py, serde_json::to_value(expanded).map_err(to_py_err)?)
    })
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
    let buy_json = request_dict_to_json(buy_counts)?;
    let sell_json = request_dict_to_json(sell_counts)?;
    let buy: BTreeMap<i64, i64> = serde_json::from_value(buy_json).map_err(to_py_err)?;
    let sell: BTreeMap<i64, i64> = serde_json::from_value(sell_json).map_err(to_py_err)?;
    let aggregated = aggregate_two_sided_offer_counts(&buy, &sell, &tracked_sizes);
    Python::attach(|py| {
        dict_from_json_value(py, serde_json::to_value(aggregated).map_err(to_py_err)?)
    })
}

#[pyfunction]
#[pyo3(name = "one_sided_offer_counts_by_side")]
fn one_sided_offer_counts_by_side_py(
    sell_counts: &Bound<'_, PyDict>,
    tracked_sizes: Vec<i64>,
) -> PyResult<Py<PyAny>> {
    let sell_json = request_dict_to_json(sell_counts)?;
    let sell: BTreeMap<i64, i64> = serde_json::from_value(sell_json).map_err(to_py_err)?;
    let (buy, sell_side) = one_sided_offer_counts_by_side(&sell, &tracked_sizes);
    Python::attach(|py| {
        let dict = PyDict::new(py);
        dict.set_item(
            "buy",
            dict_from_json_value(py, serde_json::to_value(buy).map_err(to_py_err)?)?,
        )?;
        dict.set_item(
            "sell",
            dict_from_json_value(py, serde_json::to_value(sell_side).map_err(to_py_err)?)?,
        )?;
        Ok(dict.into())
    })
}

#[pyfunction]
#[pyo3(name = "merge_market_cycle_strategy_execution")]
fn merge_market_cycle_strategy_execution_py(
    result: &Bound<'_, PyDict>,
    planned: i64,
    executed: i64,
) -> PyResult<Py<PyAny>> {
    let payload = request_dict_to_json(result)?;
    let mut state: MarketCycleResultState =
        serde_json::from_value(payload).map_err(to_py_err)?;
    state.merge_strategy_execution(planned, executed);
    Python::attach(|py| {
        dict_from_json_value(py, serde_json::to_value(&state).map_err(to_py_err)?)
    })
}

#[pyfunction]
#[pyo3(name = "merge_market_cycle_cancel_policy")]
fn merge_market_cycle_cancel_policy_py(
    result: &Bound<'_, PyDict>,
    triggered: bool,
    planned: i64,
    executed: i64,
) -> PyResult<Py<PyAny>> {
    let payload = request_dict_to_json(result)?;
    let mut state: MarketCycleResultState =
        serde_json::from_value(payload).map_err(to_py_err)?;
    state.merge_cancel_policy(triggered, planned, executed);
    Python::attach(|py| {
        dict_from_json_value(py, serde_json::to_value(&state).map_err(to_py_err)?)
    })
}

#[pyfunction]
#[pyo3(name = "record_market_cycle_phase_error")]
fn record_market_cycle_phase_error_py(result: &Bound<'_, PyDict>) -> PyResult<Py<PyAny>> {
    let payload = request_dict_to_json(result)?;
    let mut state: MarketCycleResultState =
        serde_json::from_value(payload).map_err(to_py_err)?;
    state.record_phase_error();
    Python::attach(|py| {
        dict_from_json_value(py, serde_json::to_value(&state).map_err(to_py_err)?)
    })
}

#[pymodule]
fn greenfloor_signer(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(resolve_vault_context_py, m)?)?;
    m.add_function(wrap_pyfunction!(build_vault_cat_offer_py, m)?)?;
    m.add_function(wrap_pyfunction!(build_mixed_split_py, m)?)?;
    m.add_function(wrap_pyfunction!(build_bls_mixed_split_py, m)?)?;
    m.add_function(wrap_pyfunction!(build_bls_offer_py, m)?)?;
    m.add_function(wrap_pyfunction!(build_bls_xch_coin_op_py, m)?)?;
    m.add_function(wrap_pyfunction!(list_bls_cat_coins_py, m)?)?;
    m.add_function(wrap_pyfunction!(list_bls_cat_coins_by_ids_py, m)?)?;
    m.add_function(wrap_pyfunction!(broadcast_bls_spend_bundle_py, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_offer_asset_ids_py, m)?)?;
    m.add_function(wrap_pyfunction!(validate_offer_py, m)?)?;
    m.add_function(wrap_pyfunction!(encode_offer_py, m)?)?;
    m.add_function(wrap_pyfunction!(from_input_spend_bundle_py, m)?)?;
    m.add_function(wrap_pyfunction!(from_input_spend_bundle_xch_py, m)?)?;
    m.add_function(wrap_pyfunction!(load_bls_master_sk_py, m)?)?;
    m.add_function(wrap_pyfunction!(coinset_push_tx_py, m)?)?;
    m.add_function(wrap_pyfunction!(coinset_get_fee_estimate_py, m)?)?;
    m.add_function(wrap_pyfunction!(coinset_get_conservative_fee_estimate_py, m)?)?;
    m.add_function(wrap_pyfunction!(evaluate_market_py, m)?)?;
    m.add_function(wrap_pyfunction!(apply_offer_signal_py, m)?)?;
    m.add_function(wrap_pyfunction!(expand_strategy_actions_py, m)?)?;
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
    m.add_function(wrap_pyfunction!(merge_market_cycle_strategy_execution_py, m)?)?;
    m.add_function(wrap_pyfunction!(merge_market_cycle_cancel_policy_py, m)?)?;
    m.add_function(wrap_pyfunction!(record_market_cycle_phase_error_py, m)?)?;
    Ok(())
}
