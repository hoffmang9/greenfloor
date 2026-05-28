use pyo3::prelude::*;
use pyo3::types::PyDict;
use signer_core::notifications::{evaluate_low_inventory_alert, AlertEvent};

#[pyfunction]
#[pyo3(name = "evaluate_low_inventory_alert")]
fn evaluate_low_inventory_alert_py(
    py: Python<'_>,
    now_unix: i64,
    low_inventory_enabled: bool,
    program_default_threshold: i64,
    clear_hysteresis_percent: f64,
    dedup_cooldown_seconds: i64,
    market_enabled: bool,
    market_id: &str,
    ticker: &str,
    receive_address: &str,
    market_threshold: Option<i64>,
    low_watermark: i64,
    remaining: i64,
    state_is_low: bool,
    state_last_alert_at_unix: Option<i64>,
) -> PyResult<Py<PyAny>> {
    let evaluation = evaluate_low_inventory_alert(
        now_unix,
        low_inventory_enabled,
        program_default_threshold,
        clear_hysteresis_percent,
        dedup_cooldown_seconds,
        market_enabled,
        market_id,
        ticker,
        receive_address,
        market_threshold,
        low_watermark,
        remaining,
        state_is_low,
        state_last_alert_at_unix,
    );
    let state_dict = PyDict::new(py);
    state_dict.set_item("is_low", evaluation.state.is_low)?;
    state_dict.set_item("last_alert_at_unix", evaluation.state.last_alert_at_unix)?;

    let event_value: Bound<'_, PyAny> = match evaluation.event {
        None => py.None().into_bound(py),
        Some(AlertEvent {
            market_id,
            ticker,
            remaining_amount,
            receive_address,
            reason,
        }) => {
            let event_dict = PyDict::new(py);
            event_dict.set_item("market_id", market_id)?;
            event_dict.set_item("ticker", ticker)?;
            event_dict.set_item("remaining_amount", remaining_amount)?;
            event_dict.set_item("receive_address", receive_address)?;
            event_dict.set_item("reason", reason)?;
            event_dict.into_any()
        }
    };

    Ok((state_dict, event_value).into_pyobject(py)?.into())
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(evaluate_low_inventory_alert_py, m)?)?;
    Ok(())
}
