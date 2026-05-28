extern crate greenfloor_signer as signer_core;

mod coin_ops_py;
mod cycle;
mod execution_py;
mod hex_py;
mod notifications_py;
mod offer_build_py;
mod py_utils;
mod retry_py;
mod strategy_py;

use std::future::Future;
use std::path::Path;
use std::sync::OnceLock;

use chia_bls::SecretKey;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use signer_core::error::{bls_reason, broadcast_reason, BlsOp};
use signer_core::{
    broadcast_bls_spend_bundle, build_and_optionally_broadcast_vault_cat_mixed_split,
    build_bls_mixed_split_spend_bundle, build_bls_offer_spend_bundle,
    build_bls_xch_coin_op_spend_bundle, build_vault_cat_offer,
    encode_offer_from_spend_bundle_bytes, from_input_spend_bundle_bytes,
    from_input_spend_bundle_xch_bytes, get_conservative_fee_estimate, get_fee_estimate,
    list_cat_coin_summaries, list_cat_coin_summaries_by_ids, load_bls_master_secret_key,
    load_signer_config, push_tx_hex, resolve_offer_asset_ids, resolve_vault_context,
    validate_offer_structure, validate_offer_text, verify_offer_for_dexie, BlsMixedSplitRequest,
    BlsOfferRequest,
    BlsXchCoinOpRequest,
    CreateOfferRequest, MixedSplitRequest,
};

use py_utils::{dict_from_json_value, request_dict_to_json, to_py_err};
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
        dict_from_json_value(py, serde_json::to_value(&context).map_err(to_py_err)?)
    })
}

#[pyfunction]
#[pyo3(name = "build_vault_cat_offer")]
fn build_vault_cat_offer_py(config_path: &str, request: &Bound<'_, PyDict>) -> PyResult<Py<PyAny>> {
    let config = load_signer_config(Path::new(config_path)).map_err(to_py_err)?;
    let payload = request_dict_to_json(request)?;
    let offer_request: CreateOfferRequest = serde_json::from_value(payload).map_err(to_py_err)?;
    let result = runtime()
        .block_on(build_vault_cat_offer(config, offer_request))
        .map_err(to_py_err)?;
    Python::attach(|py| dict_from_json_value(py, serde_json::to_value(&result).map_err(to_py_err)?))
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
    let split_request: MixedSplitRequest = serde_json::from_value(payload).map_err(to_py_err)?;
    let result = runtime()
        .block_on(build_and_optionally_broadcast_vault_cat_mixed_split(
            config,
            split_request,
            broadcast,
        ))
        .map_err(to_py_err)?;
    Python::attach(|py| dict_from_json_value(py, serde_json::to_value(&result).map_err(to_py_err)?))
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
#[pyo3(name = "validate_offer_structure")]
fn validate_offer_structure_py(offer: &str) -> PyResult<()> {
    validate_offer_structure(offer).map_err(to_py_err)
}

#[pyfunction]
#[pyo3(name = "verify_offer_for_dexie")]
fn verify_offer_for_dexie_py(offer: &str) -> Option<String> {
    verify_offer_for_dexie(offer)
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
    m.add_function(wrap_pyfunction!(validate_offer_structure_py, m)?)?;
    m.add_function(wrap_pyfunction!(verify_offer_for_dexie_py, m)?)?;
    m.add_function(wrap_pyfunction!(encode_offer_py, m)?)?;
    m.add_function(wrap_pyfunction!(from_input_spend_bundle_py, m)?)?;
    m.add_function(wrap_pyfunction!(from_input_spend_bundle_xch_py, m)?)?;
    m.add_function(wrap_pyfunction!(load_bls_master_sk_py, m)?)?;
    m.add_function(wrap_pyfunction!(coinset_push_tx_py, m)?)?;
    m.add_function(wrap_pyfunction!(coinset_get_fee_estimate_py, m)?)?;
    m.add_function(wrap_pyfunction!(
        coinset_get_conservative_fee_estimate_py,
        m
    )?)?;
    coin_ops_py::register(m)?;
    cycle::register(m)?;
    hex_py::register(m)?;
    notifications_py::register(m)?;
    offer_build_py::register(m)?;
    retry_py::register(m)?;
    Ok(())
}
