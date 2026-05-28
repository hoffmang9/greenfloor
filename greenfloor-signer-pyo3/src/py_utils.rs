use std::collections::BTreeMap;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

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

pub fn request_dict_to_json(request: &Bound<'_, PyDict>) -> PyResult<serde_json::Value> {
    let py = request.py();
    let json_mod = py.import("json")?;
    let dumps = json_mod.getattr("dumps")?;
    let raw = dumps.call1((request,))?;
    let raw_str: String = raw.extract()?;
    serde_json::from_str(&raw_str).map_err(to_py_err)
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

pub fn extract_spendable_profiles(
    profiles: &Bound<'_, PyDict>,
) -> PyResult<BTreeMap<String, signer_core::SpendableAssetProfile>> {
    let mut map = BTreeMap::new();
    for (asset_id, value) in profiles.iter() {
        let profile = value.downcast::<PyDict>().map_err(|_| {
            PyValueError::new_err("spendable profile values must be dicts")
        })?;
        let max_single_known = profile
            .get_item("max_single_known")?
            .ok_or_else(|| {
                PyValueError::new_err("spendable profile max_single_known must be bool")
            })?
            .extract::<bool>()?;
        map.insert(
            asset_id.extract::<String>()?,
            signer_core::SpendableAssetProfile {
                total: profile
                    .get_item("total")?
                    .and_then(|item| item.extract::<i64>().ok())
                    .unwrap_or(0),
                max_single: profile
                    .get_item("max_single")?
                    .and_then(|item| item.extract::<i64>().ok())
                    .unwrap_or(0),
                max_single_known,
            },
        );
    }
    Ok(map)
}
