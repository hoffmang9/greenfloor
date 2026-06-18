use std::path::PathBuf;

use crate::runtime;

use engine_core::daemon::{
    consume_reload_marker, initialize_daemon_file_logging, reconcile_offers_cli,
    resolve_coinset_ws_url, run_daemon_cycle_once_from_json, run_daemon_loop_from_json,
    websocket_capture_enabled, DaemonInstanceLock,
};
use engine_core::offer::lifecycle::{offers_cancel_cli, offers_status_cli};
use crate::py_utils::{dict_from_json_value, request_dict_to_json, to_py_err};
use engine_core::error::SignerError;
use engine_core::storage::resolve_state_db_path;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};

pyo3::create_exception!(
    greenfloor_engine,
    DaemonLockConflict,
    PyException,
    "Raised when another greenfloord instance holds the state-dir lock."
);

fn map_daemon_lock_err(err: SignerError) -> PyErr {
    match err {
        SignerError::DaemonAlreadyRunning { .. } => DaemonLockConflict::new_err(err.to_string()),
        other => to_py_err(other),
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

#[pyfunction]
#[pyo3(name = "acquire_daemon_instance_lock", signature = (state_dir, mode, /))]
fn acquire_daemon_instance_lock_py(state_dir: PathBuf, mode: &str) -> PyResult<PyDaemonInstanceLock> {
    let lock = DaemonInstanceLock::acquire(&state_dir, mode).map_err(map_daemon_lock_err)?;
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
    let program = engine_core::config::load_program_config(&program_path).map_err(to_py_err)?;
    Ok(resolve_coinset_ws_url(&program, coinset_base_url))
}

#[pyfunction]
#[pyo3(name = "resolve_state_db_path", signature = (program_home_dir, explicit_db_path=None, /))]
fn resolve_state_db_path_py(program_home_dir: PathBuf, explicit_db_path: Option<String>) -> String {
    resolve_state_db_path(&program_home_dir, explicit_db_path.as_deref())
        .display()
        .to_string()
}

#[pyfunction]
#[pyo3(name = "use_websocket_capture_for_trigger_mode", signature = (tx_block_trigger_mode, /))]
fn use_websocket_capture_for_trigger_mode_py(tx_block_trigger_mode: &str) -> bool {
    websocket_capture_enabled(tx_block_trigger_mode)
}

#[pyfunction]
#[pyo3(name = "consume_reload_marker", signature = (state_dir, /))]
fn consume_reload_marker_py(state_dir: PathBuf) -> bool {
    consume_reload_marker(&state_dir)
}

#[pyfunction]
#[pyo3(name = "run_daemon_cycle_once", signature = (request, /))]
fn run_daemon_cycle_once_py(py: Python<'_>, request: &Bound<'_, PyDict>) -> PyResult<Py<PyAny>> {
    let payload = request_dict_to_json(request)?;
    let response = py.detach(move || {
        runtime()
            .block_on(run_daemon_cycle_once_from_json(payload))
            .map_err(to_py_err)
    })?;
    dict_from_json_value(
        py,
        serde_json::to_value(response).map_err(to_py_err)?,
    )
}

#[pyfunction]
#[pyo3(name = "run_daemon_loop", signature = (request, /))]
fn run_daemon_loop_py(py: Python<'_>, request: &Bound<'_, PyDict>) -> PyResult<i32> {
    let payload = request_dict_to_json(request)?;
    py.detach(move || {
        runtime()
            .block_on(run_daemon_loop_from_json(payload))
            .map_err(to_py_err)
    })
}

#[pyfunction]
#[pyo3(
    name = "reconcile_offers_cli",
    signature = (db_path, dexie_base_url, target_venue, market_id=None, limit=500, /)
)]
fn reconcile_offers_cli_py(
    py: Python<'_>,
    db_path: PathBuf,
    dexie_base_url: String,
    target_venue: String,
    market_id: Option<String>,
    limit: usize,
) -> PyResult<Py<PyAny>> {
    let market_filter = market_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let market_ref = market_filter.as_deref();
    let payload = py.detach(move || {
        runtime()
            .block_on(reconcile_offers_cli(
                &db_path,
                &dexie_base_url,
                &target_venue,
                market_ref,
                limit,
            ))
            .map_err(to_py_err)
    })?;
    dict_from_json_value(
        py,
        serde_json::to_value(payload).map_err(to_py_err)?,
    )
}

#[pyfunction]
#[pyo3(
    name = "offers_status_cli",
    signature = (db_path, market_id=None, limit=50, events_limit=30, /)
)]
fn offers_status_cli_py(
    py: Python<'_>,
    db_path: PathBuf,
    market_id: Option<String>,
    limit: usize,
    events_limit: usize,
) -> PyResult<Py<PyAny>> {
    let market_filter = market_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let market_ref = market_filter.as_deref();
    let payload = offers_status_cli(&db_path, market_ref, limit, events_limit).map_err(to_py_err)?;
    dict_from_json_value(
        py,
        serde_json::to_value(payload).map_err(to_py_err)?,
    )
}

#[pyfunction]
#[pyo3(
    name = "offers_cancel_cli",
    signature = (db_path, dexie_base_url, target_venue, offer_ids, cancel_open=false, /)
)]
fn offers_cancel_cli_py(
    py: Python<'_>,
    db_path: PathBuf,
    dexie_base_url: String,
    target_venue: String,
    offer_ids: Vec<String>,
    cancel_open: bool,
) -> PyResult<Py<PyAny>> {
    let payload = py.detach(move || {
        runtime()
            .block_on(offers_cancel_cli(
                &db_path,
                &dexie_base_url,
                &target_venue,
                &offer_ids,
                cancel_open,
            ))
            .map_err(to_py_err)
    })?;
    dict_from_json_value(
        py,
        serde_json::to_value(payload).map_err(to_py_err)?,
    )
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(consume_reload_marker_py, m)?)?;
    m.add_function(wrap_pyfunction!(reconcile_offers_cli_py, m)?)?;
    m.add_function(wrap_pyfunction!(offers_status_cli_py, m)?)?;
    m.add_function(wrap_pyfunction!(offers_cancel_cli_py, m)?)?;
    m.add_function(wrap_pyfunction!(run_daemon_cycle_once_py, m)?)?;
    m.add_function(wrap_pyfunction!(run_daemon_loop_py, m)?)?;
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
    m.add_function(wrap_pyfunction!(resolve_state_db_path_py, m)?)?;
    m.add_class::<PyDaemonInstanceLock>()?;
    m.add("DaemonLockConflict", m.py().get_type::<DaemonLockConflict>())?;
    Ok(())
}
