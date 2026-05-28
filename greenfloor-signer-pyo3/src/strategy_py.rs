use std::collections::BTreeMap;

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use signer_core::{
    evaluate_market, evaluate_two_sided_market_actions, plan_reseed_actions_from_gap,
    MarketState, PlannedAction, StrategyConfig,
};

use crate::py_utils::{dict_to_i64_i64_map, planned_action_class};

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

pub fn planned_action_from_py(obj: &Bound<'_, PyAny>) -> PyResult<PlannedAction> {
    Ok(PlannedAction {
        size: obj.getattr("size")?.extract()?,
        repeat: obj.getattr("repeat")?.extract()?,
        pair: obj.getattr("pair")?.extract()?,
        expiry_unit: obj.getattr("expiry_unit")?.extract()?,
        expiry_value: obj.getattr("expiry_value")?.extract()?,
        cancel_after_create: obj.getattr("cancel_after_create")?.extract()?,
        reason: obj.getattr("reason")?.extract()?,
        target_spread_bps: match obj.getattr("target_spread_bps") {
            Ok(value) if value.is_none() => None,
            Ok(value) => Some(value.extract()?),
            Err(_) => None,
        },
        side: obj
            .getattr("side")
            .ok()
            .and_then(|value| value.extract::<String>().ok())
            .unwrap_or_else(|| "sell".to_string()),
    })
}

pub fn planned_action_to_py<'py>(
    py: Python<'py>,
    action: &PlannedAction,
) -> PyResult<Bound<'py, PyAny>> {
    let planned_action_cls = planned_action_class(py)?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("size", action.size)?;
    kwargs.set_item("repeat", action.repeat)?;
    kwargs.set_item("pair", &action.pair)?;
    kwargs.set_item("expiry_unit", &action.expiry_unit)?;
    kwargs.set_item("expiry_value", action.expiry_value)?;
    kwargs.set_item("cancel_after_create", action.cancel_after_create)?;
    kwargs.set_item("reason", &action.reason)?;
    match action.target_spread_bps {
        Some(value) => kwargs.set_item("target_spread_bps", value)?,
        None => kwargs.set_item("target_spread_bps", py.None())?,
    }
    kwargs.set_item("side", &action.side)?;
    planned_action_cls.call((), Some(&kwargs))
}

pub fn planned_actions_to_py_list(py: Python<'_>, actions: &[PlannedAction]) -> PyResult<Py<PyAny>> {
    let list = PyList::empty(py);
    for action in actions {
        list.append(planned_action_to_py(py, action)?)?;
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

#[pyfunction]
#[pyo3(name = "plan_reseed_actions_from_gap")]
fn plan_reseed_actions_from_gap_py(
    strategy_actions: &Bound<'_, PyList>,
    active_counts_by_size: &Bound<'_, PyDict>,
    target_counts_by_size: &Bound<'_, PyDict>,
    seed_candidates: &Bound<'_, PyList>,
) -> PyResult<Py<PyAny>> {
    let mut parsed_strategy_actions = Vec::with_capacity(strategy_actions.len());
    for item in strategy_actions.iter() {
        parsed_strategy_actions.push(planned_action_from_py(&item)?);
    }
    let active_counts_by_size = dict_to_i64_i64_map(active_counts_by_size)?;
    let target_counts_by_size = dict_to_i64_i64_map(target_counts_by_size)?;
    let mut parsed_seed_candidates = Vec::with_capacity(seed_candidates.len());
    for item in seed_candidates.iter() {
        parsed_seed_candidates.push(planned_action_from_py(&item)?);
    }
    let plan = plan_reseed_actions_from_gap(
        &parsed_strategy_actions,
        &active_counts_by_size,
        &target_counts_by_size,
        &parsed_seed_candidates,
    );
    Python::attach(|py| {
        let result = PyDict::new(py);
        result.set_item(
            "actions",
            planned_actions_to_py_list(py, &plan.actions)?,
        )?;
        match plan.skip_reason {
            Some(reason) => result.set_item("skip_reason", reason.label())?,
            None => result.set_item("skip_reason", py.None())?,
        }
        Ok(result.into())
    })
}

pub fn register_strategy(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(evaluate_market_typed_py, m)?)?;
    m.add_function(wrap_pyfunction!(evaluate_two_sided_market_actions_py, m)?)?;
    m.add_function(wrap_pyfunction!(plan_reseed_actions_from_gap_py, m)?)?;
    Ok(())
}
