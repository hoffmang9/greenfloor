use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyList};

use signer_core::{
    amount_meets_coin_op_min_mojos, coin_op_min_amount_mojos, coin_op_should_stop,
    coin_op_target_amount_allowed, compute_bucket_counts_from_coins, evaluate_coin_split_gate,
    fee_budget_allows_execution, is_spendable_wallet_coin, partition_plans_by_budget,
    plan_auto_combine_inputs, plan_auto_split_selection, plan_coin_ops,
    projected_coin_ops_fee_mojos, select_spendable_coins_for_target_amount,
    split_would_create_sub_cat_change,
};

use crate::py_utils::{
    bucket_spec_from_py, coin_op_plan_to_py, coin_op_plans_from_py_list,
    combine_input_selection_mode_from_py, dict_from_json_value, exclude_coin_ids_from_py_optional,
    i64_i64_map_to_py_dict, request_dict_to_json, spendable_coins_from_py_list,
    split_auto_select_plan_to_py, split_planning_profile_from_py, to_py_err,
};

fn coin_amount_mojos_from_py(coin: &Bound<'_, PyDict>) -> PyResult<Option<i64>> {
    match coin.get_item("amount")? {
        None => Ok(Some(0)),
        Some(value) => match value.extract::<i64>() {
            Ok(amount) => Ok(Some(amount)),
            Err(_) => Ok(None),
        },
    }
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
    let bucket_specs: Vec<_> = buckets
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
    let amount = match coin_amount_mojos_from_py(coin)? {
        Some(amount) => amount,
        None => return Ok(false),
    };
    Ok(amount_meets_coin_op_min_mojos(amount, canonical_asset_id))
}

#[pyfunction]
#[pyo3(name = "coin_op_target_amount_allowed")]
fn coin_op_target_amount_allowed_py(amount_mojos: i64, canonical_asset_id: &str) -> bool {
    coin_op_target_amount_allowed(amount_mojos, canonical_asset_id)
}

#[pyfunction]
#[pyo3(name = "select_spendable_coins_for_target_amount")]
fn select_spendable_coins_for_target_amount_py(
    py: Python<'_>,
    coins: &Bound<'_, PyList>,
    target_amount: i64,
) -> PyResult<Py<PyAny>> {
    let parsed = spendable_coins_from_py_list(coins)?;
    let (coin_ids, total, exact) = select_spendable_coins_for_target_amount(&parsed, target_amount);
    let ids = PyList::new(py, &coin_ids)?;
    Ok((ids, total, exact).into_pyobject(py)?.into())
}

#[pyfunction]
#[pyo3(name = "split_would_create_sub_cat_change")]
fn split_would_create_sub_cat_change_py(
    selected_amount_mojos: i64,
    required_amount_mojos: i64,
    canonical_asset_id: &str,
) -> (bool, i64) {
    split_would_create_sub_cat_change(
        selected_amount_mojos,
        required_amount_mojos,
        canonical_asset_id,
    )
}

#[pyfunction]
#[pyo3(name = "plan_auto_split_selection")]
fn plan_auto_split_selection_py(
    py: Python<'_>,
    candidate_spendable: &Bound<'_, PyList>,
    required_amount_mojos: i64,
    canonical_asset_id: &str,
    profile: &Bound<'_, PyAny>,
    combine_input_cap: i64,
    allow_combine_prereq: Option<bool>,
) -> PyResult<Py<PyAny>> {
    let profile = split_planning_profile_from_py(profile)?;
    let coins = spendable_coins_from_py_list(candidate_spendable)?;
    let plan = plan_auto_split_selection(
        &coins,
        required_amount_mojos,
        canonical_asset_id,
        profile,
        combine_input_cap,
        allow_combine_prereq,
    );
    Ok(split_auto_select_plan_to_py(py, plan)?.into())
}

#[pyfunction]
#[pyo3(name = "plan_auto_combine_inputs")]
fn plan_auto_combine_inputs_py(
    py: Python<'_>,
    spendable_coins: &Bound<'_, PyList>,
    number_of_coins: usize,
    selection_mode: &Bound<'_, PyAny>,
    target_amount_mojos: Option<i64>,
    exclude_coin_ids: Option<&Bound<'_, PyAny>>,
    max_count: Option<usize>,
) -> PyResult<Py<PyAny>> {
    let mode = combine_input_selection_mode_from_py(selection_mode)?;
    let coins = spendable_coins_from_py_list(spendable_coins)?;
    let excluded = exclude_coin_ids_from_py_optional(exclude_coin_ids)?;
    let ids = plan_auto_combine_inputs(
        &coins,
        number_of_coins,
        mode,
        target_amount_mojos,
        Some(&excluded),
        max_count,
    )
    .map_err(PyValueError::new_err)?;
    let list = PyList::new(py, &ids)?;
    Ok(list.into())
}

fn wallet_coins_from_py_list(coins: &Bound<'_, PyList>) -> PyResult<Vec<serde_json::Value>> {
    coins
        .iter()
        .map(|item| {
            let dict = item.downcast::<PyDict>()?;
            request_dict_to_json(dict)
        })
        .collect()
}

#[pyfunction]
#[pyo3(name = "is_spendable_wallet_coin")]
fn is_spendable_wallet_coin_py(coin: &Bound<'_, PyDict>) -> PyResult<bool> {
    let value = request_dict_to_json(coin)?;
    Ok(is_spendable_wallet_coin(&value))
}

#[pyfunction]
#[pyo3(name = "evaluate_coin_split_gate")]
fn evaluate_coin_split_gate_py(
    py: Python<'_>,
    asset_scoped_coins: &Bound<'_, PyList>,
    resolved_asset_id: &str,
    size_base_units: i64,
    required_count: i64,
) -> PyResult<Py<PyAny>> {
    let coins = wallet_coins_from_py_list(asset_scoped_coins)?;
    let gate = evaluate_coin_split_gate(
        &coins,
        resolved_asset_id,
        size_base_units,
        required_count,
    );
    dict_from_json_value(py, serde_json::to_value(&gate).map_err(to_py_err)?)
}

#[pyfunction]
#[pyo3(name = "coin_op_should_stop")]
fn coin_op_should_stop_py(
    until_ready: bool,
    final_readiness_ready: Option<bool>,
    has_explicit_coin_ids: bool,
    iteration: i64,
    max_iterations: i64,
) -> PyResult<(bool, String)> {
    let (stop, reason) = coin_op_should_stop(
        until_ready,
        final_readiness_ready,
        has_explicit_coin_ids,
        iteration,
        max_iterations,
    );
    Ok((stop, reason.to_string()))
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
    m.add_function(wrap_pyfunction!(
        select_spendable_coins_for_target_amount_py,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(split_would_create_sub_cat_change_py, m)?)?;
    m.add_function(wrap_pyfunction!(plan_auto_split_selection_py, m)?)?;
    m.add_function(wrap_pyfunction!(plan_auto_combine_inputs_py, m)?)?;
    m.add_function(wrap_pyfunction!(is_spendable_wallet_coin_py, m)?)?;
    m.add_function(wrap_pyfunction!(evaluate_coin_split_gate_py, m)?)?;
    m.add_function(wrap_pyfunction!(coin_op_should_stop_py, m)?)?;
    Ok(())
}
