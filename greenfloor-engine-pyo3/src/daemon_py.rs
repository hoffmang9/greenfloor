use std::path::PathBuf;

use engine_core::daemon::{
    run_daemon_cycle_once, DaemonCycleTestControls, DaemonDispatchState, DaemonRunOnceRequest,
};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use serde::Deserialize;

use crate::py_utils::{dict_from_json_value, request_dict_to_json, to_py_err};
use crate::runtime;

#[derive(Debug, Clone, Default, Deserialize)]
struct DaemonDispatchStatePy {
    #[serde(default)]
    cursor: usize,
    #[serde(default)]
    immediate_requeue_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct DaemonCycleTestControlsPy {
    #[serde(default)]
    skip_strategy_execution: bool,
    #[serde(default)]
    force_market_error_for: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct DaemonRunOnceRequestPy {
    program_path: PathBuf,
    markets_path: PathBuf,
    #[serde(default)]
    testnet_markets_path: Option<PathBuf>,
    #[serde(default)]
    state_db_override: Option<String>,
    #[serde(default)]
    coinset_base_url: String,
    state_dir: PathBuf,
    #[serde(default = "default_true")]
    poll_coinset_mempool: bool,
    #[serde(default)]
    use_websocket_capture: bool,
    #[serde(default)]
    allowed_key_ids: Vec<String>,
    #[serde(default)]
    dispatch_state: DaemonDispatchStatePy,
    #[serde(default)]
    test_controls: DaemonCycleTestControlsPy,
}

fn default_true() -> bool {
    true
}

fn parse_request(request: &Bound<'_, PyAny>) -> PyResult<DaemonRunOnceRequestPy> {
    let dict = request.downcast::<PyDict>()?;
    let payload = request_dict_to_json(dict)?;
    serde_json::from_value(payload).map_err(to_py_err)
}

fn engine_request_from_py(parsed: DaemonRunOnceRequestPy) -> DaemonRunOnceRequest {
    DaemonRunOnceRequest {
        program_path: parsed.program_path,
        markets_path: parsed.markets_path,
        testnet_markets_path: parsed.testnet_markets_path,
        state_db_override: parsed.state_db_override,
        coinset_base_url: parsed.coinset_base_url,
        state_dir: parsed.state_dir,
        poll_coinset_mempool: parsed.poll_coinset_mempool,
        use_websocket_capture: parsed.use_websocket_capture,
        allowed_key_ids: parsed.allowed_key_ids,
        dispatch_state: DaemonDispatchState {
            cursor: parsed.dispatch_state.cursor,
            immediate_requeue_ids: parsed.dispatch_state.immediate_requeue_ids,
        },
        test_controls: DaemonCycleTestControls {
            skip_strategy_execution: parsed.test_controls.skip_strategy_execution,
            force_market_error_for: parsed.test_controls.force_market_error_for,
        },
    }
}

#[pyfunction]
#[pyo3(name = "run_daemon_cycle_once", signature = (request, /))]
fn run_daemon_cycle_once_py(py: Python<'_>, request: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    let parsed = parse_request(request)?;
    let engine_request = engine_request_from_py(parsed);

    let response = py.detach(move || {
        runtime()
            .block_on(run_daemon_cycle_once(&engine_request))
            .map_err(to_py_err)
    })?;

    Python::attach(|py| {
        let out = PyDict::new(py);
        out.set_item("exit_code", response.exit_code)?;
        out.set_item(
            "dispatch_state",
            dict_from_json_value(
                py,
                serde_json::to_value(&response.dispatch_state).map_err(to_py_err)?,
            )?,
        )?;
        let summary_value = serde_json::to_value(&response.cycle_summary).map_err(to_py_err)?;
        out.set_item("cycle_summary", dict_from_json_value(py, summary_value)?)?;
        Ok(out.into())
    })
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(run_daemon_cycle_once_py, m)?)?;
    Ok(())
}
