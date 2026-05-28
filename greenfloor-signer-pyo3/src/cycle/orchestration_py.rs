use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use signer_core::{
    MarketBatchSelection, OfferStateRow, StaleSweepCandidate, StaleSweepHit, StaleSweepProgress,
};

use crate::py_utils::{
    market_batch_selection_class, stale_sweep_candidate_class, stale_sweep_hit_class,
    stale_sweep_progress_class,
};

pub fn market_batch_selection_to_py<'py>(
    py: Python<'py>,
    selection: &MarketBatchSelection,
) -> PyResult<Bound<'py, PyAny>> {
    let cls = market_batch_selection_class(py)?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("selected_market_ids", &selection.selected_market_ids)?;
    kwargs.set_item("consumed_immediate_requeues", &selection.consumed_immediate_requeues)?;
    kwargs.set_item("cursor", selection.cursor)?;
    kwargs.set_item("immediate_requeue_ids", &selection.immediate_requeue_ids)?;
    cls.call((), Some(&kwargs))
}

pub fn stale_sweep_candidates_to_py_list(
    py: Python<'_>,
    candidates: &[StaleSweepCandidate],
) -> PyResult<Py<PyAny>> {
    let cls = stale_sweep_candidate_class(py)?;
    let list = PyList::empty(py);
    for candidate in candidates {
        let kwargs = PyDict::new(py);
        kwargs.set_item("market_id", &candidate.market_id)?;
        kwargs.set_item("offer_id", &candidate.offer_id)?;
        list.append(cls.call((), Some(&kwargs))?)?;
    }
    Ok(list.into())
}

pub fn stale_sweep_hit_to_py<'py>(
    py: Python<'py>,
    hit: &StaleSweepHit,
) -> PyResult<Bound<'py, PyAny>> {
    let cls = stale_sweep_hit_class(py)?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("market_id", &hit.market_id)?;
    kwargs.set_item("offer_id", &hit.offer_id)?;
    kwargs.set_item("reason", &hit.reason)?;
    cls.call((), Some(&kwargs))
}

pub fn stale_sweep_progress_to_py<'py>(
    py: Python<'py>,
    progress: &StaleSweepProgress,
) -> PyResult<Bound<'py, PyAny>> {
    let cls = stale_sweep_progress_class(py)?;
    let hits = PyList::empty(py);
    for hit in &progress.hits {
        hits.append(stale_sweep_hit_to_py(py, hit)?)?;
    }
    let requeue = PyList::empty(py);
    for market_id in &progress.requeue_market_ids {
        requeue.append(market_id)?;
    }
    let kwargs = PyDict::new(py);
    kwargs.set_item("checked_offer_count", progress.checked_offer_count)?;
    kwargs.set_item("requeue_market_ids", requeue)?;
    kwargs.set_item("hits", hits)?;
    kwargs.set_item("truncated", progress.truncated)?;
    cls.call((), Some(&kwargs))
}

pub fn stale_sweep_progress_from_py(obj: &Bound<'_, PyAny>) -> PyResult<StaleSweepProgress> {
    let checked_offer_count = obj.getattr("checked_offer_count")?.extract::<usize>()?;
    let truncated = obj.getattr("truncated")?.extract::<bool>()?;
    let requeue_attr = obj.getattr("requeue_market_ids")?;
    let requeue_list = requeue_attr.downcast::<PyList>()?;
    let mut requeue_market_ids = Vec::with_capacity(requeue_list.len());
    for item in requeue_list.iter() {
        requeue_market_ids.push(item.extract::<String>()?);
    }
    let hits_attr = obj.getattr("hits")?;
    let hits_list = hits_attr.downcast::<PyList>()?;
    let mut hits = Vec::with_capacity(hits_list.len());
    for item in hits_list.iter() {
        hits.push(stale_sweep_hit_from_py(&item)?);
    }
    Ok(StaleSweepProgress {
        checked_offer_count,
        requeue_market_ids,
        hits,
        truncated,
    })
}

pub fn stale_sweep_hit_from_py(obj: &Bound<'_, PyAny>) -> PyResult<StaleSweepHit> {
    Ok(StaleSweepHit {
        market_id: obj.getattr("market_id")?.extract::<String>()?,
        offer_id: obj.getattr("offer_id")?.extract::<String>()?,
        reason: obj.getattr("reason")?.extract::<String>()?,
    })
}

pub fn offer_state_row_from_py(obj: &Bound<'_, PyAny>) -> PyResult<OfferStateRow> {
    Ok(OfferStateRow {
        market_id: obj.getattr("market_id")?.extract::<String>()?,
        offer_id: obj.getattr("offer_id")?.extract::<String>()?,
        state: obj.getattr("state")?.extract::<String>()?,
    })
}

pub fn parallel_action_outcomes_from_py_list(
    items: &Bound<'_, PyList>,
) -> PyResult<Vec<(String, bool)>> {
    let mut pairs = Vec::with_capacity(items.len());
    for item in items.iter() {
        let status = item.getattr("status")?.extract::<String>()?;
        let transient_upstream = item
            .getattr("transient_upstream")?
            .extract::<bool>()
            .unwrap_or(false);
        pairs.push((status, transient_upstream));
    }
    Ok(pairs)
}
