use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::py_utils::{dict_to_i64_i64_map, i64_i64_map_to_py_dict};

use signer_core::{
    aggregate_two_sided_offer_counts, is_two_sided_market_mode, one_sided_offer_counts_by_side,
    resolve_tracked_sizes,
};

#[pyfunction]
#[pyo3(name = "resolve_tracked_sizes")]
fn resolve_tracked_sizes_py(ladder_sizes: Vec<i64>, strategy_default_sizes: Vec<i64>) -> Vec<i64> {
    resolve_tracked_sizes(&ladder_sizes, &strategy_default_sizes)
}

#[pyfunction]
#[pyo3(name = "is_two_sided_market_mode")]
fn is_two_sided_market_mode_py(market_mode: &str) -> bool {
    is_two_sided_market_mode(market_mode)
}

#[pyfunction]
#[pyo3(name = "aggregate_two_sided_offer_counts")]
fn aggregate_two_sided_offer_counts_py(
    buy_counts: &Bound<'_, PyDict>,
    sell_counts: &Bound<'_, PyDict>,
    tracked_sizes: Vec<i64>,
) -> PyResult<Py<PyAny>> {
    let buy = dict_to_i64_i64_map(buy_counts)?;
    let sell = dict_to_i64_i64_map(sell_counts)?;
    let aggregated = aggregate_two_sided_offer_counts(&buy, &sell, &tracked_sizes);
    Python::attach(|py| Ok(i64_i64_map_to_py_dict(py, &aggregated)?.into()))
}

#[pyfunction]
#[pyo3(name = "one_sided_offer_counts_by_side")]
fn one_sided_offer_counts_by_side_py(
    sell_counts: &Bound<'_, PyDict>,
    tracked_sizes: Vec<i64>,
) -> PyResult<Py<PyAny>> {
    let sell = dict_to_i64_i64_map(sell_counts)?;
    let (buy, sell_side) = one_sided_offer_counts_by_side(&sell, &tracked_sizes);
    Python::attach(|py| {
        let dict = PyDict::new(py);
        dict.set_item("buy", i64_i64_map_to_py_dict(py, &buy)?)?;
        dict.set_item("sell", i64_i64_map_to_py_dict(py, &sell_side)?)?;
        Ok(dict.into())
    })
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(resolve_tracked_sizes_py, m)?)?;
    m.add_function(wrap_pyfunction!(is_two_sided_market_mode_py, m)?)?;
    m.add_function(wrap_pyfunction!(aggregate_two_sided_offer_counts_py, m)?)?;
    m.add_function(wrap_pyfunction!(one_sided_offer_counts_by_side_py, m)?)?;
    Ok(())
}
