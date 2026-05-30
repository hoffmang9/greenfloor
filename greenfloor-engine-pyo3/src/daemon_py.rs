use std::path::PathBuf;

use crate::runtime;

use engine_core::load_program_config;
use engine_core::daemon::{
    initialize_daemon_file_logging, resolve_coinset_ws_url, run_daemon_cycle_once,
    start_coinset_websocket_loop, websocket_capture_enabled, DaemonCycleOnceResponse,
    DaemonCycleTestControls, DaemonDispatchState, DaemonInstanceLock, DaemonRunOnceRequest,
};
use pyo3::prelude::*;
use pyo3::types::PyModule;

use crate::py_utils::{dict_from_json_value, to_py_err};

#[pyclass(name = "DaemonDispatchState")]
#[derive(Clone)]
struct PyDaemonDispatchState {
    #[pyo3(get, set)]
    cursor: usize,
    #[pyo3(get, set)]
    immediate_requeue_ids: Vec<String>,
}

#[pymethods]
impl PyDaemonDispatchState {
    #[new]
    #[pyo3(signature = (cursor=0, immediate_requeue_ids=None))]
    fn new(cursor: usize, immediate_requeue_ids: Option<Vec<String>>) -> Self {
        Self {
            cursor,
            immediate_requeue_ids: immediate_requeue_ids.unwrap_or_default(),
        }
    }
}

impl From<PyDaemonDispatchState> for DaemonDispatchState {
    fn from(value: PyDaemonDispatchState) -> Self {
        Self {
            cursor: value.cursor,
            immediate_requeue_ids: value.immediate_requeue_ids,
        }
    }
}

impl From<DaemonDispatchState> for PyDaemonDispatchState {
    fn from(value: DaemonDispatchState) -> Self {
        Self {
            cursor: value.cursor,
            immediate_requeue_ids: value.immediate_requeue_ids,
        }
    }
}

#[pyclass(name = "DaemonCycleTestControls")]
#[derive(Clone, Default)]
struct PyDaemonCycleTestControls {
    #[pyo3(get, set)]
    skip_strategy_execution: bool,
    #[pyo3(get, set)]
    force_market_error_for: Option<String>,
}

#[pymethods]
impl PyDaemonCycleTestControls {
    #[new]
    #[pyo3(signature = (skip_strategy_execution=false, force_market_error_for=None))]
    fn new(skip_strategy_execution: bool, force_market_error_for: Option<String>) -> Self {
        Self {
            skip_strategy_execution,
            force_market_error_for,
        }
    }
}

impl From<PyDaemonCycleTestControls> for DaemonCycleTestControls {
    fn from(value: PyDaemonCycleTestControls) -> Self {
        Self {
            skip_strategy_execution: value.skip_strategy_execution,
            force_market_error_for: value.force_market_error_for,
        }
    }
}

#[pyclass(name = "DaemonRunOnceRequest")]
#[derive(Clone)]
struct PyDaemonRunOnceRequest {
    #[pyo3(get, set)]
    program_path: PathBuf,
    #[pyo3(get, set)]
    markets_path: PathBuf,
    #[pyo3(get, set)]
    testnet_markets_path: Option<PathBuf>,
    #[pyo3(get, set)]
    state_db_override: Option<String>,
    #[pyo3(get, set)]
    coinset_base_url: String,
    #[pyo3(get, set)]
    state_dir: PathBuf,
    #[pyo3(get, set)]
    poll_coinset_mempool: bool,
    #[pyo3(get, set)]
    use_websocket_capture: bool,
    #[pyo3(get, set)]
    allowed_key_ids: Vec<String>,
    #[pyo3(get, set)]
    dispatch_state: PyDaemonDispatchState,
    #[pyo3(get, set)]
    test_controls: PyDaemonCycleTestControls,
}

#[pymethods]
impl PyDaemonRunOnceRequest {
    #[allow(clippy::too_many_arguments)]
    #[new]
    #[pyo3(signature = (
        program_path,
        markets_path,
        coinset_base_url,
        state_dir,
        *,
        testnet_markets_path=None,
        state_db_override=None,
        poll_coinset_mempool=true,
        use_websocket_capture=false,
        allowed_key_ids=None,
        dispatch_state=None,
        test_controls=None,
    ))]
    fn new(
        program_path: PathBuf,
        markets_path: PathBuf,
        coinset_base_url: String,
        state_dir: PathBuf,
        testnet_markets_path: Option<PathBuf>,
        state_db_override: Option<String>,
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
            test_controls: test_controls.unwrap_or_default(),
        }
    }
}

impl From<PyDaemonRunOnceRequest> for DaemonRunOnceRequest {
    fn from(value: PyDaemonRunOnceRequest) -> Self {
        Self {
            program_path: value.program_path,
            markets_path: value.markets_path,
            testnet_markets_path: value.testnet_markets_path,
            state_db_override: value.state_db_override,
            coinset_base_url: value.coinset_base_url,
            state_dir: value.state_dir,
            poll_coinset_mempool: value.poll_coinset_mempool,
            use_websocket_capture: value.use_websocket_capture,
            allowed_key_ids: value.allowed_key_ids,
            dispatch_state: value.dispatch_state.into(),
            test_controls: value.test_controls.into(),
        }
    }
}

#[pyclass(name = "DaemonCycleOnceResponse")]
struct PyDaemonCycleOnceResponse {
    #[pyo3(get)]
    exit_code: i32,
    #[pyo3(get)]
    dispatch_state: PyDaemonDispatchState,
    #[pyo3(get)]
    cycle_summary: Py<PyAny>,
}

impl PyDaemonCycleOnceResponse {
    fn from_engine(py: Python<'_>, response: DaemonCycleOnceResponse) -> PyResult<Self> {
        Ok(Self {
            exit_code: response.exit_code,
            dispatch_state: response.dispatch_state.into(),
            cycle_summary: dict_from_json_value(
                py,
                serde_json::to_value(response.cycle_summary).map_err(to_py_err)?,
            )?,
        })
    }
}

#[pyclass(name = "DaemonInstanceLock", unsendable)]
struct PyDaemonInstanceLock {
    inner: Option<DaemonInstanceLock>,
}

#[pymethods]
impl PyDaemonInstanceLock {
    fn __enter__(slf: PyRefMut<'_, Self>) -> PyResult<PyRefMut<'_, Self>> {
        Ok(slf)
    }

    #[pyo3(signature = (_exc_type=None, _exc_val=None, _exc_tb=None))]
    fn __exit__(
        mut slf: PyRefMut<'_, Self>,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_val: Option<&Bound<'_, PyAny>>,
        _exc_tb: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        slf.inner.take();
        Ok(())
    }
}

#[pyclass(name = "CoinsetWebsocketLoop", unsendable)]
struct PyCoinsetWebsocketLoop {
    inner: Option<engine_core::daemon::CoinsetWebsocketLoopHandle>,
}

#[pymethods]
impl PyCoinsetWebsocketLoop {
    fn stop(&mut self) {
        if let Some(handle) = self.inner.take() {
            handle.stop();
        }
    }
}

#[pyfunction]
#[pyo3(name = "acquire_daemon_instance_lock", signature = (state_dir, mode, /))]
fn acquire_daemon_instance_lock_py(state_dir: PathBuf, mode: &str) -> PyResult<PyDaemonInstanceLock> {
    let lock = DaemonInstanceLock::acquire(&state_dir, mode).map_err(to_py_err)?;
    Ok(PyDaemonInstanceLock {
        inner: Some(lock),
    })
}

#[pyfunction]
#[pyo3(name = "initialize_daemon_file_logging", signature = (home_dir, log_level, /))]
fn initialize_daemon_file_logging_py(home_dir: PathBuf, log_level: &str) -> PyResult<()> {
    initialize_daemon_file_logging(&home_dir, log_level).map_err(to_py_err)
}

#[pyfunction]
#[pyo3(name = "warn_if_daemon_log_level_auto_healed", signature = (log_level_was_missing, program_config_path, /))]
fn warn_if_daemon_log_level_auto_healed_py(
    log_level_was_missing: bool,
    program_config_path: PathBuf,
) {
    engine_core::daemon::warn_if_daemon_log_level_auto_healed(
        log_level_was_missing,
        &program_config_path,
    );
}

#[pyfunction]
#[pyo3(name = "resolve_coinset_ws_url", signature = (program_path, coinset_base_url, /))]
fn resolve_coinset_ws_url_py(program_path: PathBuf, coinset_base_url: &str) -> PyResult<String> {
    let program = load_program_config(&program_path).map_err(to_py_err)?;
    Ok(resolve_coinset_ws_url(&program, coinset_base_url))
}

#[pyfunction]
#[pyo3(name = "start_coinset_websocket_loop", signature = (db_path, program_path, coinset_base_url, /))]
fn start_coinset_websocket_loop_py(
    db_path: PathBuf,
    program_path: PathBuf,
    coinset_base_url: &str,
) -> PyResult<PyCoinsetWebsocketLoop> {
    let program = load_program_config(&program_path).map_err(to_py_err)?;
    let handle = start_coinset_websocket_loop(db_path, program, coinset_base_url.to_string());
    Ok(PyCoinsetWebsocketLoop {
        inner: Some(handle),
    })
}

#[pyfunction]
#[pyo3(name = "use_websocket_capture_for_trigger_mode", signature = (tx_block_trigger_mode, /))]
fn use_websocket_capture_for_trigger_mode_py(tx_block_trigger_mode: &str) -> bool {
    websocket_capture_enabled(tx_block_trigger_mode)
}

#[pyfunction]
#[pyo3(name = "run_daemon_cycle_once", signature = (request, /))]
fn run_daemon_cycle_once_py(
    py: Python<'_>,
    request: PyRef<'_, PyDaemonRunOnceRequest>,
) -> PyResult<Py<PyDaemonCycleOnceResponse>> {
    let engine_request: DaemonRunOnceRequest = request.clone().into();
    let response = py.detach(move || {
        runtime()
            .block_on(run_daemon_cycle_once(&engine_request))
            .map_err(to_py_err)
    })?;
    Py::new(
        py,
        PyDaemonCycleOnceResponse::from_engine(py, response)?,
    )
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(run_daemon_cycle_once_py, m)?)?;
    m.add_function(wrap_pyfunction!(
        use_websocket_capture_for_trigger_mode_py,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(acquire_daemon_instance_lock_py, m)?)?;
    m.add_function(wrap_pyfunction!(initialize_daemon_file_logging_py, m)?)?;
    m.add_function(wrap_pyfunction!(
        warn_if_daemon_log_level_auto_healed_py,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(resolve_coinset_ws_url_py, m)?)?;
    m.add_function(wrap_pyfunction!(start_coinset_websocket_loop_py, m)?)?;
    m.add_class::<PyDaemonInstanceLock>()?;
    m.add_class::<PyCoinsetWebsocketLoop>()?;
    m.add_class::<PyDaemonDispatchState>()?;
    m.add_class::<PyDaemonCycleTestControls>()?;
    m.add_class::<PyDaemonRunOnceRequest>()?;
    m.add_class::<PyDaemonCycleOnceResponse>()?;
    Ok(())
}
