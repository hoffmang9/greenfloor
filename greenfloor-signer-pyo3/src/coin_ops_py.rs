use std::sync::OnceLock;

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyModule};

use signer_core::{
    coin_meets_coin_op_min_amount, coin_op_min_amount_mojos, coin_op_target_amount_allowed,
    compute_bucket_counts_from_coins, fee_budget_allows_execution, partition_plans_by_budget,
    plan_coin_ops, projected_coin_ops_fee_mojos, BucketSpec, CoinOpPlan,
};

use crate::py_utils::i64_i64_map_to_py_dict;

const COIN_OPS_MODULE: &str = "greenfloor.core.coin_ops";

static BUCKET_SPEC_CLS: OnceLock<Py<PyAny>> = OnceLock::new();
static COIN_OP_PLAN_CLS: OnceLock<Py<PyAny>> = OnceLock::new();

fn cached_coin_ops_class<'py>(
    py: Python<'py>,
    cache: &OnceLock<Py<PyAny>>,
    name: &str,
) -> PyResult<Bound<'py, PyAny>> {
    if let Some(cls) = cache.get() {
        return Ok(cls.bind(py).clone());
    }
    let cls = PyModule::import(py, COIN_OPS_MODULE)?.getattr(name)?.unbind();
    let _ = cache.set(cls);
    Ok(cache.get().expect("cached class").bind(py).clone())
}

fn bucket_spec_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_coin_ops_class(py, &BUCKET_SPEC_CLS, "BucketSpec")
}

fn coin_op_plan_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_coin_ops_class(py, &COIN_OP_PLAN_CLS, "CoinOpPlan")
}

fn bucket_spec_from_py(obj: &Bound<'_, PyAny>) -> PyResult<BucketSpec> {
    if let Ok(cls) = bucket_spec_class(obj.py()) {
        if obj.is_instance(&cls)? {
            return Ok(BucketSpec {
                size_base_units: obj.getattr("size_base_units")?.extract()?,
                target_count: obj.getattr("target_count")?.extract()?,
                split_buffer_count: obj.getattr("split_buffer_count")?.extract()?,
                combine_when_excess_factor: obj.getattr("combine_when_excess_factor")?.extract()?,
                current_count: obj.getattr("current_count")?.extract()?,
            });
        }
    }
    Err(PyTypeError::new_err("expected BucketSpec"))
}

fn coin_op_plan_from_py(obj: &Bound<'_, PyAny>) -> PyResult<CoinOpPlan> {
    if let Ok(cls) = coin_op_plan_class(obj.py()) {
        if obj.is_instance(&cls)? {
            return Ok(CoinOpPlan {
                op_type: obj.getattr("op_type")?.extract()?,
                size_base_units: obj.getattr("size_base_units")?.extract()?,
                op_count: obj.getattr("op_count")?.extract()?,
                reason: obj.getattr("reason")?.extract()?,
            });
        }
    }
    Err(PyTypeError::new_err("expected CoinOpPlan"))
}

fn coin_op_plan_to_py<'py>(py: Python<'py>, plan: &CoinOpPlan) -> PyResult<Bound<'py, PyAny>> {
    let cls = coin_op_plan_class(py)?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("op_type", &plan.op_type)?;
    kwargs.set_item("size_base_units", plan.size_base_units)?;
    kwargs.set_item("op_count", plan.op_count)?;
    kwargs.set_item("reason", &plan.reason)?;
    cls.call((), Some(&kwargs))
}

fn coin_op_plans_from_py_list(plans: &Bound<'_, PyList>) -> PyResult<Vec<CoinOpPlan>> {
    let mut parsed = Vec::with_capacity(plans.len());
    for item in plans.iter() {
        parsed.push(coin_op_plan_from_py(&item)?);
    }
    Ok(parsed)
}

#[pyfunction]
#[pyo3(name = "plan_coin_ops")]
fn plan_coin_ops_py(
    py: Python<'_>,
    buckets: &Bound<'_, PyList>,
    max_operations_per_run: i64,
    max_fee_budget_mojos: i64,
    split_fee_mojos: i64,
    combine_fee_mojos: i64,
) -> PyResult<Py<PyAny>> {
    let bucket_specs: Vec<BucketSpec> = buckets
        .iter()
        .map(|item| bucket_spec_from_py(&item))
        .collect::<PyResult<_>>()?;
    let plans = plan_coin_ops(
        &bucket_specs,
        max_operations_per_run,
        max_fee_budget_mojos,
        split_fee_mojos,
        combine_fee_mojos,
    );
    let list = PyList::empty(py);
    for plan in plans {
        list.append(coin_op_plan_to_py(py, &plan)?)?;
    }
    Ok(list.into())
}

#[pyfunction]
#[pyo3(name = "projected_coin_ops_fee_mojos")]
fn projected_coin_ops_fee_mojos_py(
    plans: &Bound<'_, PyList>,
    split_fee_mojos: i64,
    combine_fee_mojos: i64,
) -> PyResult<i64> {
    let parsed = coin_op_plans_from_py_list(plans)?;
    Ok(projected_coin_ops_fee_mojos(
        &parsed,
        split_fee_mojos,
        combine_fee_mojos,
    ))
}

#[pyfunction]
#[pyo3(name = "fee_budget_allows_execution")]
fn fee_budget_allows_execution_py(
    max_daily_fee_budget_mojos: i64,
    spent_today_mojos: i64,
    projected_mojos: i64,
) -> bool {
    fee_budget_allows_execution(
        max_daily_fee_budget_mojos,
        spent_today_mojos,
        projected_mojos,
    )
}

#[pyfunction]
#[pyo3(name = "partition_plans_by_budget")]
fn partition_plans_by_budget_py(
    py: Python<'_>,
    plans: &Bound<'_, PyList>,
    split_fee_mojos: i64,
    combine_fee_mojos: i64,
    spent_today_mojos: i64,
    max_daily_fee_budget_mojos: i64,
) -> PyResult<Py<PyAny>> {
    let parsed = coin_op_plans_from_py_list(plans)?;
    let (allowed, skipped) = partition_plans_by_budget(
        &parsed,
        split_fee_mojos,
        combine_fee_mojos,
        spent_today_mojos,
        max_daily_fee_budget_mojos,
    );
    let allowed_list = PyList::empty(py);
    for plan in allowed {
        allowed_list.append(coin_op_plan_to_py(py, &plan)?)?;
    }
    let skipped_list = PyList::empty(py);
    for plan in skipped {
        skipped_list.append(coin_op_plan_to_py(py, &plan)?)?;
    }
    Ok((allowed_list, skipped_list).into_pyobject(py)?.into())
}

#[pyfunction]
#[pyo3(name = "compute_bucket_counts_from_coins")]
fn compute_bucket_counts_from_coins_py(
    py: Python<'_>,
    coin_amounts_base_units: Vec<i64>,
    ladder_sizes: Vec<i64>,
) -> PyResult<Py<PyAny>> {
    let counts = compute_bucket_counts_from_coins(&coin_amounts_base_units, &ladder_sizes);
    Ok(i64_i64_map_to_py_dict(py, &counts)?.into())
}

#[pyfunction]
#[pyo3(name = "coin_op_min_amount_mojos")]
fn coin_op_min_amount_mojos_py(canonical_asset_id: &str) -> i64 {
    coin_op_min_amount_mojos(canonical_asset_id)
}

#[pyfunction]
#[pyo3(name = "coin_meets_coin_op_min_amount")]
fn coin_meets_coin_op_min_amount_py(
    coin: &Bound<'_, PyDict>,
    canonical_asset_id: &str,
) -> PyResult<bool> {
    let amount = match coin.get_item("amount")? {
        Some(value) => value.extract::<i64>().unwrap_or(0),
        None => 0,
    };
    Ok(coin_meets_coin_op_min_amount(amount, canonical_asset_id))
}

#[pyfunction]
#[pyo3(name = "coin_op_target_amount_allowed")]
fn coin_op_target_amount_allowed_py(amount_mojos: i64, canonical_asset_id: &str) -> bool {
    coin_op_target_amount_allowed(amount_mojos, canonical_asset_id)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(plan_coin_ops_py, m)?)?;
    m.add_function(wrap_pyfunction!(projected_coin_ops_fee_mojos_py, m)?)?;
    m.add_function(wrap_pyfunction!(fee_budget_allows_execution_py, m)?)?;
    m.add_function(wrap_pyfunction!(partition_plans_by_budget_py, m)?)?;
    m.add_function(wrap_pyfunction!(compute_bucket_counts_from_coins_py, m)?)?;
    m.add_function(wrap_pyfunction!(coin_op_min_amount_mojos_py, m)?)?;
    m.add_function(wrap_pyfunction!(coin_meets_coin_op_min_amount_py, m)?)?;
    m.add_function(wrap_pyfunction!(coin_op_target_amount_allowed_py, m)?)?;
    Ok(())
}
