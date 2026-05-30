use std::path::PathBuf;

use engine_core::daemon::{
    default_bridge, run_daemon_cycle_once, DaemonDispatchState, DaemonRunOnceRequest,
};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use serde::Deserialize;

use crate::py_utils::{dict_from_json_value, request_dict_to_json, to_py_err};
use crate::runtime;

#[derive(Debug, Deserialize)]
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
    dispatch_state: DaemonDispatchState,
}

fn default_true() -> bool {
    true
}

#[pyfunction]
#[pyo3(name = "run_daemon_cycle_once")]
fn run_daemon_cycle_once_py(request: &Bound<'_, PyDict>) -> PyResult<Py<PyAny>> {
    let payload = request_dict_to_json(request)?;
    let parsed: DaemonRunOnceRequestPy = serde_json::from_value(payload).map_err(to_py_err)?;
    let engine_request = DaemonRunOnceRequest {
        program_path: parsed.program_path,
        markets_path: parsed.markets_path,
        testnet_markets_path: parsed.testnet_markets_path,
        state_db_override: parsed.state_db_override,
        coinset_base_url: parsed.coinset_base_url,
        state_dir: parsed.state_dir,
        poll_coinset_mempool: parsed.poll_coinset_mempool,
        use_websocket_capture: parsed.use_websocket_capture,
        allowed_key_ids: parsed.allowed_key_ids,
        dispatch_state: parsed.dispatch_state,
    };

    let bridge = default_bridge().map_err(to_py_err)?;
    let response = runtime()
        .block_on(run_daemon_cycle_once(&engine_request, &bridge))
        .map_err(to_py_err)?;

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
        out.set_item("cycle_summary", dict_from_json_value(py, response.cycle_summary)?)?;
        Ok(out.into())
    })
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(run_daemon_cycle_once_py, m)?)?;
    Ok(())
}
