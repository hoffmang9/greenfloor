use std::path::PathBuf;

use engine_core::manager::{build_and_post_offer, BuildAndPostOfferRequest};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde::Deserialize;

use crate::py_utils::{dict_from_json_value, request_dict_to_json, to_py_err};

#[derive(Debug, Deserialize)]
struct BuildAndPostOfferRequestPy {
    program_path: PathBuf,
    markets_path: PathBuf,
    #[serde(default)]
    testnet_markets_path: Option<PathBuf>,
    network: String,
    #[serde(default)]
    market_id: Option<String>,
    #[serde(default)]
    pair: Option<String>,
    size_base_units: u64,
    #[serde(default = "default_repeat")]
    repeat: u32,
    #[serde(default)]
    publish_venue: Option<String>,
    #[serde(default)]
    dexie_base_url: Option<String>,
    #[serde(default)]
    splash_base_url: Option<String>,
    #[serde(default = "default_true")]
    drop_only: bool,
    #[serde(default)]
    claim_rewards: bool,
    #[serde(default)]
    dry_run: bool,
    #[serde(default)]
    compact_json: bool,
    #[serde(default = "default_true")]
    persist_results: bool,
    #[serde(default)]
    action_side: Option<String>,
}

fn default_repeat() -> u32 {
    1
}

fn default_true() -> bool {
    true
}

fn build_request(payload: BuildAndPostOfferRequestPy) -> PyResult<BuildAndPostOfferRequest> {
    if payload.market_id.is_none() == payload.pair.is_none() {
        return Err(to_py_err(engine_core::Error::Other(
            "provide exactly one of market_id or pair".to_string(),
        )));
    }
    Ok(BuildAndPostOfferRequest {
        program_path: payload.program_path,
        markets_path: payload.markets_path,
        testnet_markets_path: payload.testnet_markets_path,
        network: payload.network,
        market_id: payload.market_id,
        pair: payload.pair,
        size_base_units: payload.size_base_units,
        repeat: payload.repeat,
        publish_venue: payload.publish_venue,
        dexie_base_url: payload.dexie_base_url,
        splash_base_url: payload.splash_base_url,
        drop_only: payload.drop_only,
        claim_rewards: payload.claim_rewards,
        dry_run: payload.dry_run,
        compact_json: payload.compact_json,
        persist_results: payload.persist_results,
        action_side: payload.action_side,
    })
}

#[pyfunction]
#[pyo3(name = "build_and_post_offer")]
fn build_and_post_offer_py(request: &Bound<'_, PyDict>) -> PyResult<Py<PyAny>> {
    let payload = request_dict_to_json(request)?;
    let parsed: BuildAndPostOfferRequestPy = serde_json::from_value(payload).map_err(to_py_err)?;
    let engine_request = build_request(parsed)?;
    let response = crate::runtime()
        .block_on(build_and_post_offer(engine_request))
        .map_err(to_py_err)?;
    Python::attach(|py| {
        let result = PyDict::new(py);
        result.set_item("exit_code", response.exit_code)?;
        result.set_item("payload", dict_from_json_value(py, response.payload)?)?;
        Ok(result.into())
    })
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(build_and_post_offer_py, m)?)?;
    Ok(())
}
