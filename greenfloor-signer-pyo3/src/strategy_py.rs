use std::collections::BTreeMap;

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use signer_core::{evaluate_market, evaluate_two_sided_market_actions, MarketState, PlannedAction, StrategyConfig};

use crate::py_utils::dict_to_i64_i64_map;

fn optional_i64_i64_map<'py>(
    obj: &Bound<'py, PyAny>,
    attr: &str,
) -> PyResult<Option<BTreeMap<i64, i64>>> {
    let value = match obj.getattr(attr) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    if value.is_none() {
        return Ok(None);
    }
    let dict = value.downcast::<PyDict>()?;
    Ok(Some(dict_to_i64_i64_map(dict)?))
}

fn market_state_from_py(obj: &Bound<'_, PyAny>) -> PyResult<MarketState> {
    Ok(MarketState {
        ones: obj.getattr("ones")?.extract()?,
        tens: obj.getattr("tens")?.extract()?,
        hundreds: obj.getattr("hundreds")?.extract()?,
        xch_price_usd: match obj.getattr("xch_price_usd") {
            Ok(value) if value.is_none() => None,
            Ok(value) => Some(value.extract()?),
            Err(_) => None,
        },
        bucket_counts_by_size: optional_i64_i64_map(obj, "bucket_counts_by_size")?,
    })
}

fn strategy_config_from_py(obj: &Bound<'_, PyAny>) -> PyResult<StrategyConfig> {
    Ok(StrategyConfig {
        pair: obj.getattr("pair")?.extract()?,
        ones_target: obj.getattr("ones_target")?.extract()?,
        tens_target: obj.getattr("tens_target")?.extract()?,
        hundreds_target: obj.getattr("hundreds_target")?.extract()?,
        target_spread_bps: match obj.getattr("target_spread_bps") {
            Ok(value) if value.is_none() => None,
            Ok(value) => Some(value.extract()?),
            Err(_) => None,
        },
        min_xch_price_usd: match obj.getattr("min_xch_price_usd") {
            Ok(value) if value.is_none() => None,
            Ok(value) => Some(value.extract()?),
            Err(_) => None,
        },
        max_xch_price_usd: match obj.getattr("max_xch_price_usd") {
            Ok(value) if value.is_none() => None,
            Ok(value) => Some(value.extract()?),
            Err(_) => None,
        },
        offer_expiry_minutes: match obj.getattr("offer_expiry_minutes") {
            Ok(value) if value.is_none() => None,
            Ok(value) => Some(value.extract()?),
            Err(_) => None,
        },
        target_counts_by_size: optional_i64_i64_map(obj, "target_counts_by_size")?,
    })
}

pub fn planned_action_to_py_dict<'py>(
    py: Python<'py>,
    action: &PlannedAction,
) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("size", action.size)?;
    dict.set_item("repeat", action.repeat)?;
    dict.set_item("pair", &action.pair)?;
    dict.set_item("expiry_unit", &action.expiry_unit)?;
    dict.set_item("expiry_value", action.expiry_value)?;
    dict.set_item("cancel_after_create", action.cancel_after_create)?;
    dict.set_item("reason", &action.reason)?;
    match action.target_spread_bps {
        Some(value) => dict.set_item("target_spread_bps", value)?,
        None => dict.set_item("target_spread_bps", py.None())?,
    }
    dict.set_item("side", &action.side)?;
    Ok(dict)
}

pub fn planned_actions_to_py_list(py: Python<'_>, actions: &[PlannedAction]) -> PyResult<Py<PyAny>> {
    let list = pyo3::types::PyList::empty(py);
    for action in actions {
        list.append(planned_action_to_py_dict(py, action)?)?;
    }
    Ok(list.into())
}

#[pyfunction]
#[pyo3(name = "evaluate_market")]
fn evaluate_market_typed_py(state: &Bound<'_, PyAny>, config: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    let state = market_state_from_py(state)?;
    let config = strategy_config_from_py(config)?;
    let actions = evaluate_market(&state, &config);
    Python::attach(|py| planned_actions_to_py_list(py, &actions))
}

#[pyfunction]
#[pyo3(name = "evaluate_two_sided_market_actions")]
fn evaluate_two_sided_market_actions_py(
    buy_state: &Bound<'_, PyAny>,
    sell_state: &Bound<'_, PyAny>,
    buy_config: &Bound<'_, PyAny>,
    sell_config: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let buy_state = market_state_from_py(buy_state)?;
    let sell_state = market_state_from_py(sell_state)?;
    let buy_config = strategy_config_from_py(buy_config)?;
    let sell_config = strategy_config_from_py(sell_config)?;
    let actions = evaluate_two_sided_market_actions(
        &buy_state,
        &sell_state,
        &buy_config,
        &sell_config,
    );
    Python::attach(|py| planned_actions_to_py_list(py, &actions))
}

pub fn register_strategy(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(evaluate_market_typed_py, m)?)?;
    m.add_function(wrap_pyfunction!(evaluate_two_sided_market_actions_py, m)?)?;
    Ok(())
}

fn extract_boolish(value: Option<Bound<'_, PyAny>>) -> bool {
    let Some(value) = value else {
        return false;
    };
    if let Ok(flag) = value.extract::<bool>() {
        return flag;
    }
    value.extract::<i64>().ok().is_some_and(|raw| raw != 0)
}

pub fn extract_spendable_profiles(
    profiles: &Bound<'_, PyDict>,
) -> PyResult<BTreeMap<String, signer_core::SpendableAssetProfile>> {
    let mut map = BTreeMap::new();
    for (asset_id, value) in profiles.iter() {
        let profile = value.downcast::<PyDict>().map_err(|_| {
            PyTypeError::new_err("spendable profile values must be dicts")
        })?;
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
                max_single_known: extract_boolish(profile.get_item("max_single_known")?),
            },
        );
    }
    Ok(map)
}
