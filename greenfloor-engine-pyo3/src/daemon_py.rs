use std::path::PathBuf;

use crate::runtime;

use engine_core::daemon::{
    run_daemon_cycle_once, use_websocket_capture_for_once, DaemonCycleSummary,
    DaemonCycleTestControls, DaemonDispatchState, DaemonProgramRuntime, DaemonRunOnceRequest,
};
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAnyMethods, PyDict, PyModule};

use crate::py_utils::{request_dict_to_json, to_py_err};

#[pyclass(name = "DaemonDispatchState")]
#[derive(Clone, Debug)]
pub struct PyDaemonDispatchState {
    #[pyo3(get, set)]
    pub cursor: usize,
    #[pyo3(get, set)]
    pub immediate_requeue_ids: Vec<String>,
}

#[pymethods]
impl PyDaemonDispatchState {
    #[new]
    #[pyo3(signature = (*, cursor=0, immediate_requeue_ids=None))]
    fn new(cursor: usize, immediate_requeue_ids: Option<Vec<String>>) -> Self {
        Self {
            cursor,
            immediate_requeue_ids: immediate_requeue_ids.unwrap_or_default(),
        }
    }
}

#[pyclass(name = "DaemonCycleTestControls")]
#[derive(Clone, Debug)]
pub struct PyDaemonCycleTestControls {
    #[pyo3(get, set)]
    pub skip_strategy_execution: bool,
    #[pyo3(get, set)]
    pub force_market_error_for: Option<String>,
}

#[pymethods]
impl PyDaemonCycleTestControls {
    #[new]
    #[pyo3(signature = (*, skip_strategy_execution=false, force_market_error_for=None))]
    fn new(skip_strategy_execution: bool, force_market_error_for: Option<String>) -> Self {
        Self {
            skip_strategy_execution,
            force_market_error_for,
        }
    }
}

#[pyclass(name = "DaemonRunOnceRequest", from_py_object)]
#[derive(Clone, Debug)]
pub struct PyDaemonRunOnceRequest {
    #[pyo3(get, set)]
    pub program_path: PathBuf,
    #[pyo3(get, set)]
    pub markets_path: PathBuf,
    #[pyo3(get, set)]
    pub testnet_markets_path: Option<PathBuf>,
    #[pyo3(get, set)]
    pub state_db_override: Option<String>,
    #[pyo3(get, set)]
    pub coinset_base_url: String,
    #[pyo3(get, set)]
    pub state_dir: PathBuf,
    #[pyo3(get, set)]
    pub poll_coinset_mempool: bool,
    #[pyo3(get, set)]
    pub use_websocket_capture: bool,
    #[pyo3(get, set)]
    pub allowed_key_ids: Vec<String>,
    #[pyo3(get, set)]
    pub dispatch_state: PyDaemonDispatchState,
    #[pyo3(get, set)]
    pub test_controls: PyDaemonCycleTestControls,
}

#[pymethods]
impl PyDaemonRunOnceRequest {
    #[new]
    #[pyo3(signature = (
        *,
        program_path,
        markets_path,
        testnet_markets_path=None,
        state_db_override=None,
        coinset_base_url="https://api.coinset.org".to_string(),
        state_dir,
        poll_coinset_mempool=true,
        use_websocket_capture=false,
        allowed_key_ids=None,
        dispatch_state=None,
        test_controls=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        program_path: PathBuf,
        markets_path: PathBuf,
        testnet_markets_path: Option<PathBuf>,
        state_db_override: Option<String>,
        coinset_base_url: String,
        state_dir: PathBuf,
        poll_coinset_mempool: bool,
        use_websocket_capture: bool,
        allowed_key_ids: Option<Vec<String>>,
        dispatch_state: Option<PyDaemonDispatchState>,
        test_controls: Option<PyDaemonCycleTestControls>,
    ) -> Self {
        Self {
            program_path,
            markets_path,
            testnet_markets_path,
            state_db_override,
            coinset_base_url,
            state_dir,
            poll_coinset_mempool,
            use_websocket_capture,
            allowed_key_ids: allowed_key_ids.unwrap_or_default(),
            dispatch_state: dispatch_state.unwrap_or_else(|| PyDaemonDispatchState::new(0, None)),
            test_controls: test_controls
                .unwrap_or_else(|| PyDaemonCycleTestControls::new(false, None)),
        }
    }
}

#[pyclass(name = "DaemonCycleSummary")]
#[derive(Clone, Debug)]
pub struct PyDaemonCycleSummary {
    #[pyo3(get)]
    pub duration_ms: u64,
    #[pyo3(get)]
    pub enabled_markets: usize,
    #[pyo3(get)]
    pub markets_attempted: usize,
    #[pyo3(get)]
    pub markets_processed: u64,
    #[pyo3(get)]
    pub runtime_market_slot_count: u64,
    #[pyo3(get)]
    pub stale_open_sweep_checked_offer_count: u64,
    #[pyo3(get)]
    pub stale_open_sweep_requeue_market_ids: Vec<String>,
    #[pyo3(get)]
    pub stale_open_sweep_requeue_count: usize,
    #[pyo3(get)]
    pub stale_open_sweep_truncated: bool,
    #[pyo3(get)]
    pub immediate_requeue_market_ids: Vec<String>,
    #[pyo3(get)]
    pub immediate_requeue_count: usize,
    #[pyo3(get)]
    pub error_count: u64,
    #[pyo3(get)]
    pub strategy_planned_total: u64,
    #[pyo3(get)]
    pub strategy_executed_total: u64,
    #[pyo3(get)]
    pub cancel_triggered_count: u64,
    #[pyo3(get)]
    pub cancel_planned_total: u64,
    #[pyo3(get)]
    pub cancel_executed_total: u64,
    #[pyo3(get)]
    pub consumed_immediate_requeues: Vec<String>,
}

impl From<DaemonCycleSummary> for PyDaemonCycleSummary {
    fn from(summary: DaemonCycleSummary) -> Self {
        Self {
            duration_ms: summary.duration_ms,
            enabled_markets: summary.enabled_markets,
            markets_attempted: summary.markets_attempted,
            markets_processed: summary.markets_processed,
            runtime_market_slot_count: summary.runtime_market_slot_count,
            stale_open_sweep_checked_offer_count: summary.stale_open_sweep_checked_offer_count,
            stale_open_sweep_requeue_market_ids: summary.stale_open_sweep_requeue_market_ids,
            stale_open_sweep_requeue_count: summary.stale_open_sweep_requeue_count,
            stale_open_sweep_truncated: summary.stale_open_sweep_truncated,
            immediate_requeue_market_ids: summary.immediate_requeue_market_ids,
            immediate_requeue_count: summary.immediate_requeue_count,
            error_count: summary.error_count,
            strategy_planned_total: summary.strategy_planned_total,
            strategy_executed_total: summary.strategy_executed_total,
            cancel_triggered_count: summary.cancel_triggered_count,
            cancel_planned_total: summary.cancel_planned_total,
            cancel_executed_total: summary.cancel_executed_total,
            consumed_immediate_requeues: summary.consumed_immediate_requeues,
        }
    }
}

#[pyclass(name = "DaemonCycleOnceResponse")]
#[derive(Clone, Debug)]
pub struct PyDaemonCycleOnceResponse {
    #[pyo3(get)]
    pub exit_code: i32,
    #[pyo3(get)]
    pub dispatch_state: PyDaemonDispatchState,
    #[pyo3(get)]
    pub cycle_summary: PyDaemonCycleSummary,
}

fn engine_request_from_py(parsed: PyDaemonRunOnceRequest) -> DaemonRunOnceRequest {
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

fn parse_request_from_mapping(request: &Bound<'_, PyAny>) -> PyResult<PyDaemonRunOnceRequest> {
    let dict = request.cast::<PyDict>()?;
    let payload = request_dict_to_json(dict)?;
    let program_path = payload
        .get("program_path")
        .and_then(|value| value.as_str())
        .ok_or_else(|| PyValueError::new_err("program_path is required"))?;
    let markets_path = payload
        .get("markets_path")
        .and_then(|value| value.as_str())
        .ok_or_else(|| PyValueError::new_err("markets_path is required"))?;
    let state_dir = payload
        .get("state_dir")
        .and_then(|value| value.as_str())
        .ok_or_else(|| PyValueError::new_err("state_dir is required"))?;
    let dispatch_state = payload.get("dispatch_state").cloned().unwrap_or_default();
    let test_controls = payload.get("test_controls").cloned().unwrap_or_default();
    Ok(PyDaemonRunOnceRequest {
        program_path: PathBuf::from(program_path),
        markets_path: PathBuf::from(markets_path),
        testnet_markets_path: payload
            .get("testnet_markets_path")
            .and_then(|value| value.as_str())
            .map(PathBuf::from),
        state_db_override: payload
            .get("state_db_override")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        coinset_base_url: payload
            .get("coinset_base_url")
            .and_then(|value| value.as_str())
            .unwrap_or("https://api.coinset.org")
            .to_string(),
        state_dir: PathBuf::from(state_dir),
        poll_coinset_mempool: payload
            .get("poll_coinset_mempool")
            .and_then(|value| value.as_bool())
            .unwrap_or(true),
        use_websocket_capture: payload
            .get("use_websocket_capture")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        allowed_key_ids: payload
            .get("allowed_key_ids")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        dispatch_state: PyDaemonDispatchState {
            cursor: dispatch_state
                .get("cursor")
                .and_then(|value| value.as_u64())
                .unwrap_or(0) as usize,
            immediate_requeue_ids: dispatch_state
                .get("immediate_requeue_ids")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        },
        test_controls: PyDaemonCycleTestControls {
            skip_strategy_execution: test_controls
                .get("skip_strategy_execution")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            force_market_error_for: test_controls
                .get("force_market_error_for")
                .and_then(|value| value.as_str())
                .map(str::to_string),
        },
    })
}

fn parse_request(request: &Bound<'_, PyAny>) -> PyResult<PyDaemonRunOnceRequest> {
    if let Ok(typed) = request.extract::<PyDaemonRunOnceRequest>() {
        return Ok(typed);
    }
    if request.is_instance_of::<PyDict>() {
        return parse_request_from_mapping(request);
    }
    Err(PyTypeError::new_err(
        "run_daemon_cycle_once request must be DaemonRunOnceRequest or dict",
    ))
}

#[pyfunction]
#[pyo3(name = "use_websocket_capture_for_trigger_mode", signature = (tx_block_trigger_mode, /))]
fn use_websocket_capture_for_trigger_mode_py(tx_block_trigger_mode: &str) -> bool {
    use_websocket_capture_for_once(&DaemonProgramRuntime {
        home_dir: PathBuf::new(),
        app_log_level: String::new(),
        app_log_level_was_missing: false,
        runtime_loop_interval_seconds: 30,
        tx_block_trigger_mode: tx_block_trigger_mode.to_string(),
    })
}

#[pyfunction]
#[pyo3(name = "run_daemon_cycle_once", signature = (request, /))]
fn run_daemon_cycle_once_py(
    py: Python<'_>,
    request: &Bound<'_, PyAny>,
) -> PyResult<PyDaemonCycleOnceResponse> {
    let parsed = parse_request(request)?;
    let engine_request = engine_request_from_py(parsed);

    let response = py.detach(move || {
        runtime()
            .block_on(run_daemon_cycle_once(&engine_request))
            .map_err(to_py_err)
    })?;

    Ok(PyDaemonCycleOnceResponse {
        exit_code: response.exit_code,
        dispatch_state: PyDaemonDispatchState {
            cursor: response.dispatch_state.cursor,
            immediate_requeue_ids: response.dispatch_state.immediate_requeue_ids,
        },
        cycle_summary: response.cycle_summary.into(),
    })
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(run_daemon_cycle_once_py, m)?)?;
    m.add_function(wrap_pyfunction!(
        use_websocket_capture_for_trigger_mode_py,
        m
    )?)?;
    m.add_class::<PyDaemonDispatchState>()?;
    m.add_class::<PyDaemonCycleTestControls>()?;
    m.add_class::<PyDaemonRunOnceRequest>()?;
    m.add_class::<PyDaemonCycleSummary>()?;
    m.add_class::<PyDaemonCycleOnceResponse>()?;
    Ok(())
}
