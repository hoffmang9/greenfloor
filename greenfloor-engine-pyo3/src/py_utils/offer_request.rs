use std::sync::OnceLock;

use engine_core::offer::request::SignerOfferLegAmounts;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use super::common::cached_class;

const SIGNER_OFFER_REQUEST_MODULE: &str = "greenfloor.core.signer_offer_request";

static SIGNER_OFFER_LEG_AMOUNTS_CLS: OnceLock<Py<PyAny>> = OnceLock::new();

pub fn signer_offer_leg_amounts_class<'py>(py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
    cached_class(
        py,
        &SIGNER_OFFER_LEG_AMOUNTS_CLS,
        SIGNER_OFFER_REQUEST_MODULE,
        "SignerOfferLegAmounts",
    )
}

pub fn signer_offer_leg_amounts_to_py<'py>(
    py: Python<'py>,
    leg: &SignerOfferLegAmounts,
) -> PyResult<Bound<'py, PyAny>> {
    let cls = signer_offer_leg_amounts_class(py)?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("offer_asset_id", &leg.offer_asset_id)?;
    kwargs.set_item("request_asset_id", &leg.request_asset_id)?;
    kwargs.set_item("offer_amount_mojos", leg.offer_amount_mojos)?;
    kwargs.set_item("request_amount_mojos", leg.request_amount_mojos)?;
    cls.call((), Some(&kwargs))
}
