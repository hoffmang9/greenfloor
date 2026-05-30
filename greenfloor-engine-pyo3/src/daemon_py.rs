use std::path::PathBuf;

use crate::runtime;

use engine_core::load_program_config;
use engine_core::daemon::{
    initialize_daemon_file_logging, resolve_coinset_ws_url, run_daemon_cycle_once,
    start_coinset_websocket_loop, use_websocket_capture_for_once, DaemonInstanceLock,
    DaemonProgramRuntime, DaemonRunOnceRequest,
};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyAnyMethods, PyDict, PyModule};

use crate::py_utils::{dict_from_json_value, request_dict_to_json, to_py_err};

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
fn run_daemon_cycle_once_py(py: Python<'_>, request: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    let payload = if request.is_instance_of::<PyDict>() {
        request_dict_to_json(request.cast::<PyDict>()?)?
    } else {
        return Err(PyRuntimeError::new_err(
            "run_daemon_cycle_once request must be a dict",
        ));
    };
    let engine_request: DaemonRunOnceRequest =
        serde_json::from_value(payload).map_err(to_py_err)?;

    let response = py.detach(move || {
        runtime()
            .block_on(run_daemon_cycle_once(&engine_request))
            .map_err(to_py_err)
    })?;

    Python::attach(|py| {
        dict_from_json_value(py, serde_json::to_value(&response).map_err(to_py_err)?)
    })
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
    Ok(())
}
