use pyo3::prelude::*;

use engine_core::cycle::{
    coinset_fee_lookup_retry_sleep, dexie_invalid_offer_retry_sleep,
    dexie_invalid_offer_should_retry, moderate_retry_next_sleep, moderate_retry_sleep_seconds,
    parse_rate_limit_retry_seconds, poll_exponential_advance_sleep, poll_exponential_sleep_now,
};

#[pyfunction]
#[pyo3(name = "parse_rate_limit_retry_seconds")]
fn parse_rate_limit_retry_seconds_py(error_text: &str) -> Option<f64> {
    parse_rate_limit_retry_seconds(error_text)
}

#[pyfunction]
#[pyo3(name = "moderate_retry_sleep_seconds")]
fn moderate_retry_sleep_seconds_py(current_sleep: f64, rate_limit_wait: Option<f64>) -> f64 {
    moderate_retry_sleep_seconds(current_sleep, rate_limit_wait)
}

#[pyfunction]
#[pyo3(name = "moderate_retry_next_sleep")]
fn moderate_retry_next_sleep_py(current_sleep: f64) -> f64 {
    moderate_retry_next_sleep(current_sleep)
}

#[pyfunction]
#[pyo3(name = "dexie_invalid_offer_should_retry")]
fn dexie_invalid_offer_should_retry_py(error: &str, attempt: u32, max_attempts: u32) -> bool {
    dexie_invalid_offer_should_retry(error, attempt, max_attempts)
}

#[pyfunction]
#[pyo3(name = "dexie_invalid_offer_retry_sleep")]
fn dexie_invalid_offer_retry_sleep_py(attempt: u32, initial_sleep: f64) -> f64 {
    dexie_invalid_offer_retry_sleep(attempt, initial_sleep)
}

#[pyfunction]
#[pyo3(name = "coinset_fee_lookup_retry_sleep")]
fn coinset_fee_lookup_retry_sleep_py(attempt: u32) -> f64 {
    coinset_fee_lookup_retry_sleep(attempt)
}

#[pyfunction]
#[pyo3(name = "poll_exponential_sleep_now")]
fn poll_exponential_sleep_now_py(
    elapsed_seconds: i64,
    timeout_seconds: i64,
    sleep_seconds: f64,
    initial_sleep: f64,
    max_sleep: f64,
) -> Option<f64> {
    poll_exponential_sleep_now(
        elapsed_seconds,
        timeout_seconds,
        sleep_seconds,
        initial_sleep,
        max_sleep,
    )
}

#[pyfunction]
#[pyo3(name = "poll_exponential_advance_sleep")]
fn poll_exponential_advance_sleep_py(
    sleep_seconds: f64,
    initial_sleep: f64,
    max_sleep: f64,
    multiplier: f64,
) -> f64 {
    poll_exponential_advance_sleep(sleep_seconds, initial_sleep, max_sleep, multiplier)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(parse_rate_limit_retry_seconds_py, m)?)?;
    m.add_function(wrap_pyfunction!(moderate_retry_sleep_seconds_py, m)?)?;
    m.add_function(wrap_pyfunction!(moderate_retry_next_sleep_py, m)?)?;
    m.add_function(wrap_pyfunction!(dexie_invalid_offer_should_retry_py, m)?)?;
    m.add_function(wrap_pyfunction!(dexie_invalid_offer_retry_sleep_py, m)?)?;
    m.add_function(wrap_pyfunction!(coinset_fee_lookup_retry_sleep_py, m)?)?;
    m.add_function(wrap_pyfunction!(poll_exponential_sleep_now_py, m)?)?;
    m.add_function(wrap_pyfunction!(poll_exponential_advance_sleep_py, m)?)?;
    Ok(())
}
