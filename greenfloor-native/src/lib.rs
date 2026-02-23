use chia_protocol::{Bytes32, SpendBundle};
use chia_puzzle_types::{
    Memos,
    offer::{NotarizedPayment, Payment},
};
use chia_sdk_driver::{AssetInfo, Offer, RequestedPayments, SpendContext, decode_offer};
use chia_traits::Streamable;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyModule;

fn to_py_value_error<E: std::fmt::Display>(err: E) -> PyErr {
    PyValueError::new_err(err.to_string())
}

fn parse_bytes32(raw: &[u8], field: &str) -> PyResult<Bytes32> {
    let bytes: [u8; 32] = raw
        .try_into()
        .map_err(|_| PyValueError::new_err(format!("{field} must be 32 bytes")))?;
    Ok(Bytes32::new(bytes))
}

#[pyfunction]
fn validate_offer(offer: &str) -> PyResult<()> {
    let spend_bundle = decode_offer(offer).map_err(to_py_value_error)?;
    let mut ctx = SpendContext::new();
    Offer::from_spend_bundle(&mut ctx, &spend_bundle).map_err(to_py_value_error)?;
    Ok(())
}

#[pyfunction]
fn from_input_spend_bundle_xch(
    spend_bundle_bytes: &[u8],
    requested_payments_xch: Vec<(Vec<u8>, Vec<(Vec<u8>, u64)>)>,
) -> PyResult<Vec<u8>> {
    let spend_bundle = SpendBundle::from_bytes(spend_bundle_bytes).map_err(to_py_value_error)?;

    let mut requested_payments = RequestedPayments::new();
    for (nonce_raw, payments_raw) in requested_payments_xch {
        let nonce = parse_bytes32(&nonce_raw, "nonce")?;
        let mut payments = Vec::with_capacity(payments_raw.len());
        for (puzzle_hash_raw, amount) in payments_raw {
            let puzzle_hash = parse_bytes32(&puzzle_hash_raw, "puzzle_hash")?;
            payments.push(Payment::new(puzzle_hash, amount, Memos::None));
        }
        requested_payments
            .xch
            .push(NotarizedPayment::new(nonce, payments));
    }

    let mut ctx = SpendContext::new();
    let offer = Offer::from_input_spend_bundle(
        &mut ctx,
        spend_bundle,
        requested_payments,
        AssetInfo::new(),
    )
    .map_err(to_py_value_error)?;
    let offer_spend_bundle = offer.to_spend_bundle(&mut ctx).map_err(to_py_value_error)?;
    offer_spend_bundle.to_bytes().map_err(to_py_value_error)
}

#[pymodule]
fn greenfloor_native(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(validate_offer, m)?)?;
    m.add_function(wrap_pyfunction!(from_input_spend_bundle_xch, m)?)?;
    Ok(())
}
