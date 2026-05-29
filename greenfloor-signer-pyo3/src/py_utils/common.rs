use std::collections::BTreeMap;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde_json::Value;

pub fn to_py_err<E: std::fmt::Display>(err: E) -> PyErr {
    PyValueError::new_err(err.to_string())
}

pub fn dict_from_json_value(py: Python<'_>, value: serde_json::Value) -> PyResult<Py<PyAny>> {
    let json = serde_json::to_string(&value).map_err(to_py_err)?;
    let builtins = py.import("json")?;
    let loads = builtins.getattr("loads")?;
    let obj = loads.call1((json,))?;
    Ok(obj.unbind())
}

pub fn py_any_to_json(value: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    let py = value.py();
    let json_mod = py.import("json")?;
    let dumps = json_mod.getattr("dumps")?;
    let raw = dumps.call1((value,))?;
    let raw_str: String = raw.extract()?;
    serde_json::from_str(&raw_str).map_err(to_py_err)
}

pub fn request_dict_to_json(request: &Bound<'_, PyDict>) -> PyResult<serde_json::Value> {
    py_any_to_json(request.as_any())
}

pub fn pricing_dict_from_py(pricing: &Bound<'_, PyAny>) -> PyResult<Value> {
    if let Ok(dict) = pricing.downcast::<PyDict>() {
        return request_dict_to_json(dict);
    }
    Err(PyValueError::new_err("pricing must be a dict"))
}

pub fn dict_to_i64_i64_map(dict: &Bound<'_, PyDict>) -> PyResult<BTreeMap<i64, i64>> {
    let mut map = BTreeMap::new();
    for (key, value) in dict.iter() {
        map.insert(key.extract::<i64>()?, value.extract::<i64>()?);
    }
    Ok(map)
}

pub fn i64_i64_map_to_py_dict<'py>(
    py: Python<'py>,
    map: &BTreeMap<i64, i64>,
) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    for (key, value) in map {
        dict.set_item(*key, *value)?;
    }
    Ok(dict)
}

pub fn string_i64_map_from_py_dict(dict: &Bound<'_, PyDict>) -> PyResult<BTreeMap<String, i64>> {
    let mut map = BTreeMap::new();
    for (asset_id, amount) in dict.iter() {
        map.insert(asset_id.extract::<String>()?, amount.extract::<i64>()?);
    }
    Ok(map)
}

pub fn string_i64_map_to_py_dict<'py>(
    py: Python<'py>,
    map: &BTreeMap<String, i64>,
) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    for (key, value) in map {
        dict.set_item(key, *value)?;
    }
    Ok(dict)
}

pub(super) fn cached_class<'py>(
    py: Python<'py>,
    cache: &std::sync::OnceLock<Py<PyAny>>,
    module: &str,
    name: &str,
) -> PyResult<Bound<'py, PyAny>> {
    if let Some(cls) = cache.get() {
        return Ok(cls.bind(py).clone());
    }
    let cls = PyModule::import(py, module)?.getattr(name)?.unbind();
    let _ = cache.set(cls);
    Ok(cache.get().expect("cached class").bind(py).clone())
}
