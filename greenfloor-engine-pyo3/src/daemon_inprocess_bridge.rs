//! In-process Python bridge for PyO3 `run_daemon_cycle_once` (no subprocess hop).

use std::sync::Arc;

use engine_core::daemon::DaemonPythonBridge;
use engine_core::error::SignerResult;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde_json::Value;

use crate::py_utils::{dict_from_json_value, py_any_to_json};

const BRIDGE_MODULE: &str = "greenfloor.daemon.rust_cycle_bridge";

#[derive(Debug, Clone, Copy, Default)]
pub struct InProcessPythonBridge;

impl DaemonPythonBridge for InProcessPythonBridge {
    fn call_method(&self, method: &str, kwargs: &Value) -> SignerResult<Value> {
        Python::attach(|py| inprocess_call(py, method, kwargs)).map_err(|err| {
            engine_core::error::SignerError::Other(err.to_string())
        })
    }
}

pub fn inprocess_bridge() -> Arc<dyn DaemonPythonBridge> {
    Arc::new(InProcessPythonBridge)
}

fn inprocess_call(py: Python<'_>, method: &str, kwargs: &Value) -> PyResult<Value> {
    let module = PyModule::import(py, BRIDGE_MODULE)?;
    let func = module.getattr(method)?;
    let kwargs_obj = dict_from_json_value(py, kwargs.clone())?;
    let kwargs_dict = kwargs_obj.bind(py).downcast::<PyDict>()?;
    let result = func.call((), Some(kwargs_dict))?;
    py_any_to_json(&result)
}
