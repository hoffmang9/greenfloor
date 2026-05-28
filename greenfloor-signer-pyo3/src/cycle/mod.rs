mod inventory_py;
mod managed_py;
mod market_py;
mod offer_py;
mod stale_sweep_py;
mod strategy_counts_py;

use pyo3::prelude::*;

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    crate::strategy_py::register_strategy(m)?;
    crate::execution_py::register_execution(m)?;
    offer_py::register(m)?;
    managed_py::register(m)?;
    market_py::register(m)?;
    stale_sweep_py::register(m)?;
    inventory_py::register(m)?;
    strategy_counts_py::register(m)?;
    Ok(())
}
