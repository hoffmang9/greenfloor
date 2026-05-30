use engine_core::coinset::{
    extract_coin_id_hints_from_offer_text, list_wallet_unspent_coins, spend_bundle_hash_from_hex,
};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use crate::py_utils::to_py_err;

#[pyfunction]
#[pyo3(name = "spend_bundle_hash_hex")]
fn spend_bundle_hash_hex_py(spend_bundle_hex: &str) -> PyResult<String> {
    spend_bundle_hash_from_hex(spend_bundle_hex).map_err(to_py_err)
}

#[pyfunction]
#[pyo3(name = "extract_coin_id_hints_from_offer")]
fn extract_coin_id_hints_from_offer_py(offer_text: &str) -> PyResult<Vec<String>> {
    extract_coin_id_hints_from_offer_text(offer_text).map_err(to_py_err)
}

#[pyfunction]
#[pyo3(name = "list_wallet_unspent_coins")]
fn list_wallet_unspent_coins_py(
    network: &str,
    receive_address: &str,
    asset_id: &str,
) -> PyResult<Py<PyAny>> {
    Python::attach(|py| {
        let coins = crate::runtime()
            .block_on(list_wallet_unspent_coins(
                network,
                receive_address,
                asset_id,
            ))
            .map_err(to_py_err)?;
        let list = PyList::empty(py);
        for coin in coins {
            let dict = PyDict::new(py);
            dict.set_item("id", coin.id)?;
            dict.set_item("name", coin.name)?;
            dict.set_item("amount", coin.amount)?;
            dict.set_item("state", coin.state)?;
            list.append(dict)?;
        }
        Ok(list.into())
    })
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(spend_bundle_hash_hex_py, m)?)?;
    m.add_function(wrap_pyfunction!(extract_coin_id_hints_from_offer_py, m)?)?;
    m.add_function(wrap_pyfunction!(list_wallet_unspent_coins_py, m)?)?;
    Ok(())
}
